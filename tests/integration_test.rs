use sigmaos_display_manager::{Config, DisplayManagerComponent, Greeter, GreeterState, LoginAttempt, Component};

#[test]
fn test_greeter_initial_state_aaa() {
    let greeter = Greeter::new("seat0");
    assert_eq!(greeter.state(), &GreeterState::Waiting);
    assert_eq!(greeter.seat(), "seat0");
}

#[test]
fn test_greeter_login_success_aaa() {
    let mut greeter = Greeter::new("seat0");
    greeter.start_login(&LoginAttempt::new("alice", "wayland"));
    assert_eq!(greeter.state(), &GreeterState::LoggedIn);
}

#[test]
fn test_greeter_login_empty_user_fails_aaa() {
    let mut greeter = Greeter::new("seat0");
    greeter.start_login(&LoginAttempt::new("", "wayland"));
    assert_eq!(greeter.state(), &GreeterState::Failed);
}

#[test]
fn test_greeter_logout_aaa() {
    let mut greeter = Greeter::new("seat0");
    greeter.start_login(&LoginAttempt::new("bob", "x11"));
    greeter.logout();
    assert_eq!(greeter.state(), &GreeterState::Waiting);
}

#[test]
fn test_component_lifecycle_aaa() {
    let mut c = DisplayManagerComponent::new(Config::default());
    assert!(c.initialize().is_ok());
    assert!(c.health_check().is_ok());
    assert!(c.shutdown().is_ok());
}

#[test]
fn test_component_name_aaa() {
    let c = DisplayManagerComponent::new(Config::default());
    assert_eq!(c.name(), "sigmaos-display-manager");
}
