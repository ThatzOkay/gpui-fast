#[test]
fn shaders_translate_to_glsl_es_100() {
    use naga::back::glsl;
    let source = include_str!("shader.wgsl");
    let module = naga::front::wgsl::parse_str(source).expect("valid WGSL");
    let info = naga::valid::Validator::new(Default::default(), Default::default())
        .validate(&module)
        .expect("shader validation");
    let options = glsl::Options {
        version: glsl::Version::Embedded {
            version: 100,
            is_webgl: false,
        },
        ..Default::default()
    };
    for entry in &module.entry_points {
        let pipeline = glsl::PipelineOptions {
            shader_stage: entry.stage,
            entry_point: entry.name.clone(),
            multiview: None,
        };
        let mut output = String::new();
        glsl::Writer::new(
            &mut output,
            &module,
            &info,
            &options,
            &pipeline,
            Default::default(),
        )
        .expect("GLES 2 compatible shader")
        .write()
        .expect("GLSL ES 1.00 generation");
        assert!(output.starts_with("#version 100"));
    }
}
