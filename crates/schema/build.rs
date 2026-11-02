#[path = "../../package/icon_policy.rs"]
mod icon_policy;

fn main() {
    icon_policy::emit_rerun_inputs();
    println!("cargo:rustc-check-cfg=cfg(development_icon)");
    if icon_policy::selected_development() {
        println!("cargo:rustc-cfg=development_icon");
    }
}
