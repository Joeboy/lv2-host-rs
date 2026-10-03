#include <gtk/gtk.h>

#include <lv2/ui/ui.h>
#include <suil/suil.h>
#include <poll.h>
#include <unistd.h>
#include <cerrno>

#include <array>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <iostream>
#include <mutex>
#include <string>
#include <thread>
#include <unordered_map>
#include <vector>

namespace {

struct Control {
  uint32_t index;
  float value;
};

struct Request {
  std::string plugin_uri;
  std::string window_title;
  std::string ui_uri;
  std::string ui_type_uri;
  std::string bundle_path;
  std::string binary_path;
  std::unordered_map<std::string, uint32_t> ports;
  std::vector<Control> controls;
};

struct Controller {
  int protocol_fd;
  std::mutex output;
  std::unordered_map<std::string, uint32_t> ports;
};

bool read_exact(void* output, size_t size) {
  auto* bytes = static_cast<char*>(output);
  while (size > 0) {
    const ssize_t count = read(STDIN_FILENO, bytes, size);
    if (count < 0 && errno == EINTR) continue;
    if (count <= 0) return false;
    bytes += count;
    size -= count;
  }
  return true;
}

bool read_u32(uint32_t& value) {
  return read_exact(&value, sizeof(value));
}

bool read_string(std::string& value) {
  uint32_t size = 0;
  if (!read_u32(size) || size > 16 * 1024 * 1024) {
    return false;
  }
  value.resize(size);
  return size == 0 || read_exact(value.data(), size);
}

bool read_request(Request& request) {
  char magic[4];
  if (!read_exact(magic, sizeof(magic)) || std::memcmp(magic, "LV2G", 4) != 0) {
    return false;
  }
  if (!read_string(request.plugin_uri) ||
      !read_string(request.window_title) ||
      !read_string(request.ui_uri) ||
      !read_string(request.ui_type_uri) ||
      !read_string(request.bundle_path) ||
      !read_string(request.binary_path)) {
    return false;
  }
  uint32_t port_count = 0;
  if (!read_u32(port_count) || port_count > 65536) {
    return false;
  }
  for (uint32_t i = 0; i < port_count; ++i) {
    uint32_t index = 0;
    std::string symbol;
    if (!read_u32(index) || !read_string(symbol)) {
      return false;
    }
    request.ports.emplace(std::move(symbol), index);
  }
  uint32_t control_count = 0;
  if (!read_u32(control_count) || control_count > 65536) {
    return false;
  }
  request.controls.resize(control_count);
  for (auto& control : request.controls) {
    if (!read_u32(control.index) || !read_exact(&control.value, sizeof(control.value))) {
      return false;
    }
  }
  return true;
}

bool write_exact(int fd, const void* data, size_t size) {
  const auto* bytes = static_cast<const char*>(data);
  while (size > 0) {
    const ssize_t written = write(fd, bytes, size);
    if (written <= 0) {
      return false;
    }
    bytes += written;
    size -= written;
  }
  return true;
}

void send_ready(Controller& controller) {
  const uint8_t kind = 0;
  std::lock_guard<std::mutex> lock(controller.output);
  write_exact(controller.protocol_fd, &kind, sizeof(kind));
}

void send_error(int protocol_fd, const std::string& message) {
  const uint8_t kind = 1;
  const uint32_t size = static_cast<uint32_t>(message.size());
  write_exact(protocol_fd, &kind, sizeof(kind));
  write_exact(protocol_fd, &size, sizeof(size));
  write_exact(protocol_fd, message.data(), message.size());
}

void write_port(
    LV2UI_Controller handle,
    uint32_t index,
    uint32_t size,
    uint32_t protocol,
    const void* buffer) {
  if (!handle || !buffer || protocol != 0 || size != sizeof(float)) {
    return;
  }
  auto& controller = *static_cast<Controller*>(handle);
  const uint8_t kind = 2;
  std::lock_guard<std::mutex> lock(controller.output);
  write_exact(controller.protocol_fd, &kind, sizeof(kind));
  write_exact(controller.protocol_fd, &index, sizeof(index));
  write_exact(controller.protocol_fd, buffer, sizeof(float));
}

uint32_t port_index(LV2UI_Controller handle, const char* symbol) {
  if (!handle || !symbol) {
    return UINT32_MAX;
  }
  const auto& ports = static_cast<Controller*>(handle)->ports;
  const auto found = ports.find(symbol);
  return found == ports.end() ? UINT32_MAX : found->second;
}

}  // namespace


namespace {
struct UiState {
  Controller* controller;
  SuilInstance* instance;
  GtkWidget* window;
  const LV2UI_Idle_Interface* idle;
};

gboolean on_command(GIOChannel*, GIOCondition condition, gpointer data) {
  auto& state = *static_cast<UiState*>(data);
  if (condition & (G_IO_HUP | G_IO_ERR)) {
    gtk_main_quit();
    return G_SOURCE_REMOVE;
  }
  std::array<unsigned char, 9> packet{};
  if (!read_exact(packet.data(), packet.size())) {
    gtk_main_quit();
    return G_SOURCE_REMOVE;
  }
  if (packet[0] == 0) {
    gtk_widget_show(state.window);
    gtk_window_present(GTK_WINDOW(state.window));
  } else if (packet[0] == 1) {
    uint32_t index, bits;
    std::memcpy(&index, packet.data() + 1, sizeof(index));
    std::memcpy(&bits, packet.data() + 5, sizeof(bits));
    float value;
    std::memcpy(&value, &bits, sizeof(value));
    suil_instance_port_event(state.instance, index, sizeof(value), 0, &value);
  } else {
    gtk_main_quit();
    return G_SOURCE_REMOVE;
  }
  return G_SOURCE_CONTINUE;
}

gboolean on_idle(gpointer data) {
  auto& state = *static_cast<UiState*>(data);
  state.idle->idle(suil_instance_get_handle(state.instance));
  return G_SOURCE_CONTINUE;
}

gboolean on_delete(GtkWidget*, GdkEvent*, gpointer) {
  gtk_main_quit();
  return FALSE;
}
}  // namespace

int main(int argc, char** argv) {
  const int protocol_fd = dup(STDOUT_FILENO);
  if (protocol_fd < 0 || dup2(STDERR_FILENO, STDOUT_FILENO) < 0) return 1;
  Request request;
  if (!read_request(request)) {
    send_error(protocol_fd, "invalid GTK2 UI helper request");
    return 1;
  }
  if (!gtk_init_check(&argc, &argv)) {
    send_error(protocol_fd, "could not initialise GTK2");
    return 1;
  }
  Controller controller{protocol_fd, {}, std::move(request.ports)};
  SuilHost* host = suil_host_new(write_port, port_index, nullptr, nullptr);
  if (!host) {
    send_error(protocol_fd, "Suil could not create a GTK2 UI host");
    return 1;
  }
  const LV2_Feature make_resident = {
      "http://lv2plug.in/ns/extensions/ui#makeResident", nullptr};
  const LV2_Feature* features[] = {&make_resident, nullptr};
  SuilInstance* instance = suil_instance_new(
      host, &controller, LV2_UI__GtkUI,
      request.plugin_uri.c_str(), request.ui_uri.c_str(),
      request.ui_type_uri.c_str(), request.bundle_path.c_str(),
      request.binary_path.c_str(), features);
  if (!instance) {
    suil_host_free(host);
    send_error(protocol_fd, "Suil could not instantiate the selected GTK2 plugin UI");
    return 1;
  }
  auto* widget = static_cast<GtkWidget*>(suil_instance_get_widget(instance));
  if (!widget || !GTK_IS_WIDGET(widget)) {
    suil_instance_free(instance);
    suil_host_free(host);
    send_error(protocol_fd, "the GTK2 plugin UI did not provide a GtkWidget");
    return 1;
  }
  GtkWidget* window = gtk_window_new(GTK_WINDOW_TOPLEVEL);
  gtk_window_set_title(GTK_WINDOW(window), request.window_title.c_str());
  gtk_container_add(GTK_CONTAINER(window), widget);
  g_signal_connect(window, "delete-event", G_CALLBACK(on_delete), nullptr);
  for (const auto& control : request.controls) {
    suil_instance_port_event(instance, control.index, sizeof(float), 0, &control.value);
  }
  UiState state{&controller, instance, window, static_cast<const LV2UI_Idle_Interface*>(
      suil_instance_extension_data(instance, LV2_UI__idleInterface))};
  GIOChannel* channel = g_io_channel_unix_new(STDIN_FILENO);
  g_io_add_watch(channel, static_cast<GIOCondition>(G_IO_IN | G_IO_HUP | G_IO_ERR), on_command, &state);
  g_io_channel_unref(channel);
  if (state.idle && state.idle->idle) g_timeout_add(16, on_idle, &state);
  gtk_widget_show_all(window);
  send_ready(controller);
  gtk_main();
  gtk_container_remove(GTK_CONTAINER(window), widget);
  suil_instance_free(instance);
  suil_host_free(host);
  gtk_widget_destroy(window);
  close(protocol_fd);
  return 0;
}
