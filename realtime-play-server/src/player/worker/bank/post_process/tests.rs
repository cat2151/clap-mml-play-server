use super::*;

/// 各 sample を `gain` 倍する偽の後処理。
struct Gain(f32);

impl InstancePostProcess for Gain {
    fn process(&mut self, samples: &mut [f32], _timing: &ProcessBlockTiming) -> Result<(), String> {
        for sample in samples {
            *sample *= self.0;
        }
        Ok(())
    }
}

/// 常に失敗する偽の後処理。
struct Broken;

impl InstancePostProcess for Broken {
    fn process(
        &mut self,
        _samples: &mut [f32],
        _timing: &ProcessBlockTiming,
    ) -> Result<(), String> {
        Err("broken".to_string())
    }
}

fn timing() -> ProcessBlockTiming {
    ProcessBlockTiming {
        steady_time: 0,
        transport: None,
    }
}

fn block(value: f32) -> RenderedSamples {
    Ok(vec![value; 8])
}

/// 後処理を入れた instance の出力だけが変わり、同じ bank の他の instance はそのまま。
#[test]
fn only_the_instance_with_a_post_process_changes() {
    let mut post = InstancePostProcesses::empty(3);
    post.set(1, Some(Box::new(Gain(0.5))));

    let outputs: Vec<Vec<f32>> = (0..3)
        .map(|local_index| post.apply(local_index, block(0.8), &timing()).unwrap())
        .collect();

    assert_eq!(outputs[0], vec![0.8; 8]);
    assert_eq!(outputs[1], vec![0.4; 8]);
    assert_eq!(outputs[2], vec![0.8; 8]);
}

/// 空の置き場は render 結果を素通しする。外した後も素通しに戻る。
#[test]
fn an_empty_slot_passes_the_render_through() {
    let mut post = InstancePostProcesses::empty(2);
    assert_eq!(post.apply(0, block(0.3), &timing()).unwrap(), vec![0.3; 8]);

    post.set(0, Some(Box::new(Gain(2.0))));
    post.set(0, None);
    assert_eq!(post.apply(0, block(0.3), &timing()).unwrap(), vec![0.3; 8]);
}

/// 後処理の失敗はその instance の `Err` になり、他の instance は鳴り続ける。
/// render の失敗は後処理を通さずそのまま返る。
#[test]
fn a_failing_post_process_fails_only_its_instance() {
    let mut post = InstancePostProcesses::empty(2);
    post.set(0, Some(Box::new(Broken)));

    assert_eq!(
        post.apply(0, block(0.3), &timing()),
        Err("broken".to_string())
    );
    assert_eq!(post.apply(1, block(0.3), &timing()).unwrap(), vec![0.3; 8]);
    assert_eq!(
        post.apply(1, Err("render".to_string()), &timing()),
        Err("render".to_string())
    );
}
