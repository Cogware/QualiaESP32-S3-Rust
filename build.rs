fn main() {
    embuild::espidf::sysenv::output();
    println!("cargo:rustc-env=DEP_LV_CONFIG_PATH=lvgl_cfg");
    println!("cargo:rustc-env=LV_CONF_PATH=lvgl_cfg/lv_conf.h");
    println!("cargo:rustc-env=LV_DRV_CONF_PATH=lvgl_cfg/lv_drv_conf.h");
}
