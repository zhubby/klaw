use std::any::TypeId;

#[test]
fn klaw_runtime_exposes_cli_runtime_facade() {
    let _ = klaw_runtime::build_channel_driver_factory;
    let _ = klaw_runtime::build_hosted_runtime;
    let _ = klaw_runtime::build_runtime_bundle;
    let _ = klaw_runtime::finalize_startup_report;
    let _ = klaw_runtime::reload_runtime_skills_prompt;
    let _ = klaw_runtime::set_runtime_provider_override;
    let _ = klaw_runtime::shutdown_runtime_bundle;
    let _ = klaw_runtime::submit_and_get_output;
    let _ = klaw_runtime::submit_and_stream_output;
    let _ = klaw_runtime::sync_runtime_providers;
    let _ = klaw_runtime::sync_runtime_tools;

    let _ = TypeId::of::<klaw_runtime::GatewayStatusSnapshot>();
    let _ = TypeId::of::<klaw_runtime::HostedRuntime>();
    let _ = TypeId::of::<klaw_runtime::RuntimeBundle>();
    let _ = TypeId::of::<klaw_runtime::SharedChannelRuntime>();
    let _ = TypeId::of::<klaw_runtime::StartupReport>();
    let _ = TypeId::of::<klaw_runtime::gateway_manager::GatewayManager>();
    let _ = TypeId::of::<klaw_runtime::service_loop::BackgroundServiceConfig>();
    let _ = TypeId::of::<klaw_runtime::service_loop::BackgroundServices>();
}
