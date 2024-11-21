use core::str::Utf8Error;

use cyw43::Control;

use crate::storage::{Ssid, WifiPassword};

pub(crate) enum JoinError {
    MalformedSsid(Utf8Error),
    MalformedPassword(Utf8Error),
    JoinOpenError(cyw43::ControlError),
    JoinError(cyw43::ControlError),
}

pub(crate) async fn join_wifi(
    control: &mut Control<'_>,
    ssid: Ssid,
    password: Option<WifiPassword>,
) -> Result<(), JoinError> {
    let ssid = ssid.try_into_string().map_err(JoinError::MalformedSsid)?;

    let Some(password) = password else {
        // No password proviced, assume network is open and join
        return control
            .join_open(ssid)
            .await
            .map_err(JoinError::JoinOpenError);
    };

    let password = password
        .try_into_string()
        .map_err(JoinError::MalformedPassword)?;

    control
        .join_wpa2(ssid, password)
        .await
        .map_err(JoinError::JoinError)
}
