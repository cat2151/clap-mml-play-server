use super::*;

#[test]
fn cli_without_subcommand_runs_server() {
    assert_eq!(
        parse_cli(["clap-mml-realtime-play-server"]).unwrap(),
        CliAction::Run
    );
}

#[test]
fn update_subcommand_returns_update_action() {
    assert_eq!(
        parse_cli(["clap-mml-realtime-play-server", "update"]).unwrap(),
        CliAction::Update
    );
}

#[test]
fn check_subcommand_returns_check_action() {
    assert_eq!(
        parse_cli(["clap-mml-realtime-play-server", "check"]).unwrap(),
        CliAction::Check
    );
}

#[test]
fn probe_voicing_subcommand_parses_automation_options() {
    assert_eq!(
        parse_cli([
            "clap-mml-realtime-play-server",
            "probe-voicing",
            "--patch",
            "Leads/Mono.fxp",
            "--previous-patch",
            "Pads/Poly.fxp",
            "--json",
            "--expect",
            "mono",
        ])
        .unwrap(),
        CliAction::ProbeVoicing {
            patch: "Leads/Mono.fxp".into(),
            previous_patch: Some("Pads/Poly.fxp".into()),
            json: true,
            expect: Some(ExpectedVoicing::Mono),
        }
    );
}

#[test]
fn probe_capabilities_subcommand_parses_the_plugin_selection() {
    assert_eq!(
        parse_cli([
            "clap-mml-realtime-play-server",
            "probe-capabilities",
            "--plugin-path",
            "C:/CLAP/VASTvaporizer2.clap",
            "--plugin-id",
            "com.vastdynamics.VAST2",
            "--json",
        ])
        .unwrap(),
        CliAction::ProbeCapabilities {
            plugin_path: Some("C:/CLAP/VASTvaporizer2.clap".into()),
            plugin_id: Some("com.vastdynamics.VAST2".into()),
            json: true,
        }
    );
}

/// 引数なしなら config から引ける全プラグインを測る。この既定を落とすと
/// 「対応中のプラグインを 1 回で測り直す」使い方ができなくなる。
#[test]
fn probe_capabilities_without_a_plugin_path_targets_every_configured_plugin() {
    assert_eq!(
        parse_cli(["clap-mml-realtime-play-server", "probe-capabilities"]).unwrap(),
        CliAction::ProbeCapabilities {
            plugin_path: None,
            plugin_id: None,
            json: false,
        }
    );
}

#[test]
fn help_lists_self_update_commands_and_server_details() {
    let CliAction::PrintHelp(help) =
        parse_cli(["clap-mml-realtime-play-server", "--help"]).unwrap()
    else {
        panic!("expected help action");
    };

    assert!(help.contains("Commands:"));
    assert!(help.contains("update"));
    assert!(help.contains("check"));
    assert!(help.contains("probe-voicing"));
    assert!(help.contains("probe-capabilities"));
    assert!(help.contains("POST /play"));
    assert!(help.contains("POST /play-mml"));
    assert!(help.contains("LIVE MIDI (Windows)"));
}

#[test]
fn unknown_argument_returns_error() {
    let error = parse_cli(["clap-mml-realtime-play-server", "unknown"]).unwrap_err();

    assert!(error
        .to_string()
        .contains("unrecognized subcommand 'unknown'"));
}
