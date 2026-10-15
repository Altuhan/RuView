//! Native library of the Android POS app (`libkitepos.so`).
//!
//! `pos.ui` is compiled to Rust at build time; the JNI functions behind
//! `dev.kite.KiteView` are generated for its `Register` component.

kite::include_ui!("pos");

kite_android::kite_view_jni!(Register);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_register_runs() {
        let pos = Register::new();
        let mut host = kite_android::Host::new(pos.instance().clone());
        host.resize(1280, 800, 1.0);
        let [x, y, w, h] = host.element_rect("milk").unwrap();
        host.touch(kite_android::action::DOWN, x + w / 2.0, y + h / 2.0);
        host.touch(kite_android::action::UP, x + w / 2.0, y + h / 2.0);
        assert_eq!(pos.get_items(), 1);
        assert!((pos.get_total() - 4.63).abs() < 0.001);
    }
}
