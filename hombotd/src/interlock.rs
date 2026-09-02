//! Stage-2 motion interlocks. Fail closed.
//!
//! There is still no motor command path. This module is the gate that path
//! must consult before any future actuator call. Unknown, stale, missing, or
//! uncalibrated channels refuse motion. A `Clear` injection in tests exists
//! so the gate is not hardcoded to false -- when bumper/cliff XOR and a
//! calibrated battery later land, they flip a channel rather than inventing
//! a second policy.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Channel {
    /// Decoded and reporting the safe state.
    Clear,
    /// Decoded and reporting a trip.
    Tripped,
    /// Not decoded, stale, missing, or uncalibrated. Same as a trip for motion.
    Unknown,
}

impl Channel {
    pub(crate) fn permits_motion(self) -> bool {
        matches!(self, Channel::Clear)
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Channel::Clear => "clear",
            Channel::Tripped => "tripped",
            Channel::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Input {
    pub telemetry_fresh: bool,
    pub cliff: Channel,
    pub bumper: Channel,
    pub wheel_drop: Channel,
    pub battery: Channel,
    pub transport: Channel,
    pub charger_state_raw: Option<u8>,
    pub voltage_raw_centivolts: Option<u16>,
}

impl Input {
    /// What the live decoder can actually supply today: freshness plus the
    /// four existing battery bytes. Cliff, bumper, wheel-drop, transport, and
    /// calibrated battery are all unknown. Do not fill those from XOR guesses.
    pub(crate) fn from_live_sample(
        telemetry_fresh: bool,
        charger_state_raw: Option<u8>,
        voltage_raw_centivolts: Option<u16>,
    ) -> Self {
        Self {
            telemetry_fresh,
            cliff: Channel::Unknown,
            bumper: Channel::Unknown,
            wheel_drop: Channel::Unknown,
            battery: Channel::Unknown,
            transport: Channel::Unknown,
            charger_state_raw: telemetry_fresh.then_some(()).and(charger_state_raw),
            voltage_raw_centivolts: telemetry_fresh
                .then_some(())
                .and(voltage_raw_centivolts),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Verdict {
    pub allow_motion: bool,
    pub telemetry_fresh: bool,
    pub cliff: Channel,
    pub bumper: Channel,
    pub wheel_drop: Channel,
    pub battery: Channel,
    pub transport: Channel,
    pub reasons: Vec<&'static str>,
    pub charger_state_raw: Option<u8>,
    pub voltage_raw_centivolts: Option<u16>,
}

impl Verdict {
    pub(crate) fn json(&self) -> String {
        let charger = self
            .charger_state_raw
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_owned());
        let centivolts = self
            .voltage_raw_centivolts
            .map(|value| value.to_string())
            .unwrap_or_else(|| "null".to_owned());
        let reasons = if self.reasons.is_empty() {
            "[]".to_owned()
        } else {
            format!(
                "[{}]",
                self.reasons
                    .iter()
                    .map(|reason| format!("\"{reason}\""))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        format!(
            concat!(
                "{{\"allow_motion\":{},\"motor_path\":\"absent\",",
                "\"live_confirmed\":false,",
                "\"telemetry_fresh\":{},",
                "\"channels\":{{\"cliff\":{},\"bumper\":{},\"wheel_drop\":{},",
                "\"battery\":{},\"transport\":{}}},",
                "\"reasons\":{},",
                "\"battery\":{{\"calibration\":\"pending_multimeter_pair\",",
                "\"voltage_raw_centivolts\":{},\"charger_state_raw\":{}}}}}"
            ),
            self.allow_motion,
            self.telemetry_fresh,
            json_token(self.cliff.as_str()),
            json_token(self.bumper.as_str()),
            json_token(self.wheel_drop.as_str()),
            json_token(self.battery.as_str()),
            json_token(self.transport.as_str()),
            reasons,
            centivolts,
            charger,
        )
    }
}

fn json_token(value: &str) -> String {
    format!("\"{value}\"")
}

pub(crate) fn evaluate(input: &Input) -> Verdict {
    let mut reasons = Vec::new();
    if !input.telemetry_fresh {
        reasons.push("telemetry_stale");
    }
    push_channel(&mut reasons, "cliff", input.cliff);
    push_channel(&mut reasons, "bumper", input.bumper);
    push_channel(&mut reasons, "wheel_drop", input.wheel_drop);
    push_channel(&mut reasons, "battery", input.battery);
    push_channel(&mut reasons, "transport", input.transport);
    Verdict {
        allow_motion: reasons.is_empty(),
        telemetry_fresh: input.telemetry_fresh,
        cliff: input.cliff,
        bumper: input.bumper,
        wheel_drop: input.wheel_drop,
        battery: input.battery,
        transport: input.transport,
        reasons,
        charger_state_raw: input.charger_state_raw,
        voltage_raw_centivolts: input.voltage_raw_centivolts,
    }
}

fn push_channel(reasons: &mut Vec<&'static str>, name: &'static str, channel: Channel) {
    match channel {
        Channel::Clear => {}
        Channel::Tripped => reasons.push(name),
        Channel::Unknown => reasons.push(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_sample_refuses_while_safety_channels_are_undecoded() {
        let verdict = evaluate(&Input::from_live_sample(true, Some(1), Some(885)));
        assert!(!verdict.allow_motion);
        assert!(verdict.telemetry_fresh);
        assert_eq!(verdict.cliff, Channel::Unknown);
        assert_eq!(verdict.bumper, Channel::Unknown);
        assert_eq!(verdict.wheel_drop, Channel::Unknown);
        assert_eq!(verdict.battery, Channel::Unknown);
        assert_eq!(verdict.transport, Channel::Unknown);
        assert_eq!(
            verdict.reasons,
            ["cliff", "bumper", "wheel_drop", "battery", "transport"]
        );
        assert_eq!(verdict.voltage_raw_centivolts, Some(885));
        let json = verdict.json();
        assert!(json.contains("\"allow_motion\":false"), "{json}");
        assert!(json.contains("\"motor_path\":\"absent\""), "{json}");
        assert!(json.contains("\"calibration\":\"pending_multimeter_pair\""), "{json}");
        assert!(json.contains("\"voltage_raw_centivolts\":885"), "{json}");
        assert!(!json.contains("voltage_v"), "{json}");
    }

    #[test]
    fn stale_telemetry_is_its_own_reason_and_hides_battery_bytes() {
        let verdict = evaluate(&Input::from_live_sample(false, Some(1), Some(885)));
        assert!(!verdict.allow_motion);
        assert!(!verdict.telemetry_fresh);
        assert!(verdict.reasons.contains(&"telemetry_stale"));
        assert_eq!(verdict.voltage_raw_centivolts, None);
        assert_eq!(verdict.charger_state_raw, None);
        let json = verdict.json();
        assert!(json.contains("\"voltage_raw_centivolts\":null"), "{json}");
    }

    #[test]
    fn tripped_channel_refuses_even_when_the_rest_are_clear() {
        let verdict = evaluate(&Input {
            telemetry_fresh: true,
            cliff: Channel::Clear,
            bumper: Channel::Tripped,
            wheel_drop: Channel::Clear,
            battery: Channel::Clear,
            transport: Channel::Clear,
            charger_state_raw: Some(1),
            voltage_raw_centivolts: Some(885),
        });
        assert!(!verdict.allow_motion);
        assert_eq!(verdict.reasons, ["bumper"]);
    }

    #[test]
    fn all_clear_and_fresh_is_the_only_path_that_allows_motion() {
        let verdict = evaluate(&Input {
            telemetry_fresh: true,
            cliff: Channel::Clear,
            bumper: Channel::Clear,
            wheel_drop: Channel::Clear,
            battery: Channel::Clear,
            transport: Channel::Clear,
            charger_state_raw: Some(1),
            voltage_raw_centivolts: Some(885),
        });
        assert!(verdict.allow_motion);
        assert!(verdict.reasons.is_empty());
        assert!(verdict.json().contains("\"allow_motion\":true"));
    }

    #[test]
    fn unknown_is_not_clear() {
        assert!(!Channel::Unknown.permits_motion());
        assert!(!Channel::Tripped.permits_motion());
        assert!(Channel::Clear.permits_motion());
    }
}
