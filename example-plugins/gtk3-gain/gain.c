#include <lv2/core/lv2.h>
#include <stdlib.h>

typedef struct { const float *gain, *input; float *output; } Gain;

static LV2_Handle instantiate(const LV2_Descriptor *descriptor, double rate,
                              const char *bundle, const LV2_Feature *const *features) {
  (void)descriptor; (void)rate; (void)bundle; (void)features;
  return calloc(1, sizeof(Gain));
}
static void connect_port(LV2_Handle handle, uint32_t port, void *data) {
  Gain *gain = handle;
  if (port == 0) gain->gain = data;
  else if (port == 1) gain->input = data;
  else if (port == 2) gain->output = data;
}
static void run(LV2_Handle handle, uint32_t frames) {
  Gain *gain = handle;
  for (uint32_t i = 0; i < frames; ++i)
    gain->output[i] = gain->input[i] * *gain->gain;
}
static void cleanup(LV2_Handle handle) { free(handle); }
static const LV2_Descriptor descriptor = {
  "urn:lv2-host:example:gtk3-gain", instantiate, connect_port,
  NULL, run, NULL, cleanup, NULL
};
LV2_SYMBOL_EXPORT const LV2_Descriptor *lv2_descriptor(uint32_t index) {
  return index == 0 ? &descriptor : NULL;
}
