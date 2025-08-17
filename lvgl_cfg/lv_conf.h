// lv_conf.h — minimal config for ESP32-S3 + RGB565, LVGL v8.x
#ifndef LV_CONF_H
#define LV_CONF_H

/* Use simple include path resolution (lets LVGL find this file easily) */
#define LV_CONF_INCLUDE_SIMPLE 1

/*====================
   CORE FEATURES
 *====================*/
#define LV_COLOR_DEPTH      16      /* RGB565 */
#define LV_COLOR_16_SWAP    0       /* set 1 only if your byte order is swapped in RAM */
#define LV_HOR_RES_MAX      720
#define LV_VER_RES_MAX      720

/* Use the software renderer (normal on MCUs) */
#define LV_USE_DRAW_SW      1

/* Use default tick (we'll call lv_tick_inc() from Rust) */
#define LV_TICK_CUSTOM      0

/* Memory: adjust for your UI complexity (objects, fonts, images) */
#define LV_MEM_SIZE         (64U * 1024U)

/* Optional goodies while bringing it up */
#define LV_USE_LOG          0
#define LV_LOG_LEVEL        LV_LOG_LEVEL_WARN
#define LV_USE_PERF_MONITOR 0  /* shows CPU/fps in a corner if you enable it in code */

/* Disable things you don't use to keep build small (safe defaults) */
#define LV_USE_GPU          0
#define LV_USE_THEME_DEFAULT 1
#define LV_USE_ANTIALIAS=1

/* Fonts (enable at least one) */
#define LV_FONT_MONTSERRAT_48 1

#endif /* LV_CONF_H */