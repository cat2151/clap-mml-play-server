use super::*;

const STORAGE: &str = "const int n_fx_params = 12;";
const BASE: &str = "void Effect::init_ctrltypes() { for (int j=0;j<n_fx_params;++j) fxdata->p[j].set_type(ct_none); }";
const PARAMETER: &str = "Parameter::Parameter() { posy_offset = 0; }";

fn extract(body: &str) -> anyhow::Result<SurgeFxParamLayout> {
    extract_layout(71, "Invented".into(), LayoutSources {
        implementation: &format!("void Invented::init_ctrltypes() {{ {body} }}"),
        header: "enum invented { first, second, end }; std::string labels[2] = {\"Alpha\", \"Beta\"};",
        sst_header: None, storage: STORAGE, effect_base: BASE, parameter: PARAMETER,
    })
}

#[test]
fn derives_offsets_names_and_active_flags_without_parameter_tables() {
    let layout = extract(
        r#"
        Effect::init_ctrltypes();
        for (int i = first; i < end; ++i) {
            auto placement = 3;
            if (i >= second) placement += 4;
            fxdata->p[i].posy_offset = placement + 2 * (i / 2);
            fxdata->p[i].set_type(ct_invented);
            fxdata->p[i].set_name(labels[i].c_str());
        }
        fxdata->p[2].set_type(ct_invented);
        std::string label = "Invented " + std::to_string(17);
        fxdata->p[2].set_name(label.c_str());
    "#,
    )
    .unwrap();
    assert_eq!(&layout.posy_offset[..3], &[3, 7, 0]);
    assert_eq!(&layout.names[..3], &["Alpha", "Beta", "Invented 17"]);
    assert_eq!(&layout.active[..4], &[true, true, true, false]);
}

#[test]
fn unsupported_definitions_and_invalid_indices_fail_instead_of_guessing() {
    for body in [
        "fxdata->p[missing].set_type(ct_invented);",
        "fxdata->p[12].set_type(ct_invented);",
        "fxdata->p[0].set_name(build_name());",
        "unknown_helper();",
        "if (unknown) unknown_helper();",
        "for (int i=0;i<1;i--) fxdata->p[0].set_type(ct_invented);",
    ] {
        assert!(extract(body).is_err(), "{body}");
    }
}

#[test]
fn discovers_factory_types_and_names_from_source() {
    let effects = effect_classes(
        r#"enum fx_type { fxt_off, fxt_invented }; const int n_fx_types = 2; const char fx_type_names[2][32] = {"Off", "Invented"};"#,
        "Effect *spawn_effect(int id) { switch(id) { case fxt_invented: return new InventedEffect(storage); default: return nullptr; } }",
    ).unwrap();
    assert_eq!(effects.len(), 1);
    assert_eq!(effects[0].fx_type, 1);
    assert_eq!(effects[0].display_name, "Invented");
    assert_eq!(effects[0].class_name, "InventedEffect");
}
