fn main() {
    // no_std crates depend on kite-core directly, so point generated code at it.
    let config = kite_build::Config::default().runtime_path("::kite_core");
    kite_build::compile_with_config("ui/thermostat.ui", config);
}
