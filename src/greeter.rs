#[derive(Debug, Clone, PartialEq)]
pub enum GreeterState { Waiting, Authenticating, LoggedIn, Failed }
#[derive(Debug, Clone)]
pub struct LoginAttempt { pub username: String, pub session_type: String }
impl LoginAttempt {
    pub fn new(username: &str, session_type: &str) -> Self {
        Self { username: username.to_string(), session_type: session_type.to_string() }
    }
}
pub struct Greeter { state: GreeterState, seat: String }
impl Greeter {
    pub fn new(seat: &str) -> Self { Self { state: GreeterState::Waiting, seat: seat.to_string() } }
    pub fn start_login(&mut self, attempt: &LoginAttempt) {
        self.state = GreeterState::Authenticating;
        if !attempt.username.is_empty() { self.state = GreeterState::LoggedIn; }
        else { self.state = GreeterState::Failed; }
    }
    pub fn logout(&mut self) { self.state = GreeterState::Waiting; }
    pub fn state(&self) -> &GreeterState { &self.state }
    pub fn seat(&self) -> &str { &self.seat }
}
impl Default for Greeter { fn default() -> Self { Self::new("seat0") } }
