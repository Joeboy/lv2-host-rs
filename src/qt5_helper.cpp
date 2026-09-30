#include <QApplication>
#include <QMetaObject>
#include <QtEndian>
#include <QTimer>
#include <QWidget>

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
  if (!read_exact(magic, sizeof(magic)) || std::memcmp(magic, "LV2Q", 4) != 0) {
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

int main(int argc, char** argv) {
  const int protocol_fd = dup(STDOUT_FILENO);
  if (protocol_fd < 0 || dup2(STDERR_FILENO, STDOUT_FILENO) < 0) {
    return 1;
  }

  Request request;
  if (!read_request(request)) {
    send_error(protocol_fd, "invalid Qt5 UI helper request");
    return 1;
  }

  QApplication application(argc, argv);
  Controller controller{protocol_fd, {}, std::move(request.ports)};
  SuilHost* host = suil_host_new(write_port, port_index, nullptr, nullptr);
  if (!host) {
    send_error(protocol_fd, "Suil could not create a Qt5 UI host");
    return 1;
  }

  const LV2_Feature make_resident = {
      "http://lv2plug.in/ns/extensions/ui#makeResident", nullptr};
  const LV2_Feature* features[] = {&make_resident, nullptr};
  SuilInstance* instance = suil_instance_new(
      host,
      &controller,
      LV2_UI__Qt5UI,
      request.plugin_uri.c_str(),
      request.ui_uri.c_str(),
      request.ui_type_uri.c_str(),
      request.bundle_path.c_str(),
      request.binary_path.c_str(),
      features);
  if (!instance) {
    suil_host_free(host);
    send_error(protocol_fd, "Suil could not instantiate the selected Qt5 plugin UI");
    return 1;
  }

  auto* widget = static_cast<QWidget*>(suil_instance_get_widget(instance));
  if (!widget) {
    suil_instance_free(instance);
    suil_host_free(host);
    send_error(protocol_fd, "the Qt5 plugin UI did not provide a QWidget");
    return 1;
  }

  widget->setWindowTitle(QString::fromUtf8(request.window_title.c_str()));
  for (const auto& control : request.controls) {
    suil_instance_port_event(
        instance,
        control.index,
        sizeof(float),
        0,
        &control.value);
  }

  QTimer idle_timer;
  const auto* idle = static_cast<const LV2UI_Idle_Interface*>(
      suil_instance_extension_data(instance, LV2_UI__idleInterface));
  if (idle && idle->idle) {
    QObject::connect(&idle_timer, &QTimer::timeout, [instance, idle]() {
      idle->idle(suil_instance_get_handle(instance));
    });
    idle_timer.start(16);
  }

  widget->show();
  // Keep pipe I/O off Qt's GUI thread. Commands are posted back through Qt's
  // queued invocation mechanism, so widget and Suil calls still run there.
  std::atomic<bool> reading_commands{true};
  std::thread command_reader([&]() {
    while (reading_commands.load()) {
      pollfd pipe{STDIN_FILENO, POLLIN, 0};
      const int ready = poll(&pipe, 1, 100);
      if (ready < 0 && errno == EINTR) continue;
      if (ready == 0) continue;
      if (ready < 0 || !(pipe.revents & POLLIN)) {
        QMetaObject::invokeMethod(&application, &QApplication::quit, Qt::QueuedConnection);
        return;
      }

      std::array<unsigned char, 9> packet{};
      if (!read_exact(packet.data(), packet.size())) {
        QMetaObject::invokeMethod(&application, &QApplication::quit, Qt::QueuedConnection);
        return;
      }
      QMetaObject::invokeMethod(
          &application,
          [widget, instance, packet, &application]() {
            if (packet[0] == 0) {
              widget->show();
              widget->raise();
              widget->activateWindow();
            } else if (packet[0] == 1) {
              const uint32_t index = qFromLittleEndian<uint32_t>(packet.data() + 1);
              const uint32_t bits = qFromLittleEndian<uint32_t>(packet.data() + 5);
              float value;
              std::memcpy(&value, &bits, sizeof(value));
              suil_instance_port_event(instance, index, sizeof(value), 0, &value);
            } else {
              application.quit();
            }
          },
          Qt::QueuedConnection);
    }
  });
  send_ready(controller);
  const int result = application.exec();
  reading_commands.store(false);
  command_reader.join();

  suil_instance_free(instance);
  suil_host_free(host);
  close(protocol_fd);
  return result;
}
