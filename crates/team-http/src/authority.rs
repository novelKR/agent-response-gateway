use gateway_management::{Digest, Id, Identity};
use gateway_team_access::{Authenticator, Principal};
/// Trusted model authentication boundary. Implementations must verify credentials themselves;
/// client role/subject data is not a principal. Target is fixed for this authority instance.
pub trait ModelAuthority: Send + Sync {
    fn target(&self) -> &Id;
    fn authenticate_model(&self, credential: &str) -> Option<Principal>;
    fn refresh_model(&self, identity: &Identity, version: &Digest) -> Option<Principal>;
}
impl ModelAuthority for Authenticator {
    fn target(&self) -> &Id {
        Authenticator::target(self)
    }
    fn authenticate_model(&self, credential: &str) -> Option<Principal> {
        Authenticator::authenticate_model(self, credential)
    }
    fn refresh_model(&self, identity: &Identity, version: &Digest) -> Option<Principal> {
        Authenticator::refresh_model(self, identity, version)
    }
}
