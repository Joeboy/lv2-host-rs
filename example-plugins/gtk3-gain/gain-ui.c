#include <gtk/gtk.h>
#include <lv2/ui/ui.h>
#include <stdlib.h>

typedef struct {
  GtkWidget *scale;
  LV2UI_Write_Function write;
  LV2UI_Controller controller;
} Ui;

static void changed(GtkRange *range, gpointer data) {
  Ui *ui = data;
  float value = (float)gtk_range_get_value(range);
  ui->write(ui->controller, 0, sizeof(value), 0, &value);
}
static LV2UI_Handle instantiate(const LV2UI_Descriptor *descriptor,
                                const char *plugin_uri, const char *bundle,
                                LV2UI_Write_Function write, LV2UI_Controller controller,
                                LV2UI_Widget *widget,
                                const LV2_Feature *const *features) {
  (void)descriptor; (void)plugin_uri; (void)bundle; (void)features;
  Ui *ui = calloc(1, sizeof(Ui));
  ui->write = write;
  ui->controller = controller;
  ui->scale = gtk_scale_new_with_range(GTK_ORIENTATION_HORIZONTAL, 0.0, 2.0, 0.01);
  gtk_widget_set_size_request(ui->scale, 240, 50);
  g_signal_connect(ui->scale, "value-changed", G_CALLBACK(changed), ui);
  *widget = ui->scale;
  return ui;
}
static void cleanup(LV2UI_Handle handle) { free(handle); }
static void port_event(LV2UI_Handle handle, uint32_t port, uint32_t size,
                       uint32_t format, const void *buffer) {
  Ui *ui = handle;
  if (port == 0 && size == sizeof(float) && format == 0)
    gtk_range_set_value(GTK_RANGE(ui->scale), *(const float *)buffer);
}
static const LV2UI_Descriptor descriptor = {
  "urn:lv2-host:example:gtk3-gain-ui", instantiate, cleanup, port_event, NULL
};
LV2_SYMBOL_EXPORT const LV2UI_Descriptor *lv2ui_descriptor(uint32_t index) {
  return index == 0 ? &descriptor : NULL;
}
