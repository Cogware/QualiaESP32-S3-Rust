#ifndef LV_DRV_CONF_H
#define LV_DRV_CONF_H
#define LV_CONF_INCLUDE_SIMPLE 1
/* We don't use the C drivers; Rust flush_cb talks to esp_lcd.
   Keeping this file present satisfies lvgl-sys build script. */
#endif
