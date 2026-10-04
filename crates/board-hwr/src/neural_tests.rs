use super::*;
fn point(x: f32, y: f32) -> StrokePoint {
    StrokePoint {
        x,
        y,
        time: 0.0,
        pressure: 0.5,
    }
}
fn one_plus_one() -> Vec<Vec<StrokePoint>> {
    vec![
        vec![point(15., 25.), point(25., 15.), point(25., 85.)],
        vec![point(58., 50.), point(98., 50.)],
        vec![point(78., 30.), point(78., 70.)],
        vec![point(122., 25.), point(132., 15.), point(132., 85.)],
    ]
}
#[test]
fn preprocessing_and_limits() {
    let image = rasterize(&one_plus_one()).unwrap();
    let pixels = normalize(&image).unwrap();
    assert_eq!(pixels.len(), 448 * 448);
    assert!(pixels.iter().all(|v| v.is_finite()));
    assert!(pixels.iter().any(|v| *v < -6.0));
    assert!(pixels[447 * 448..].iter().all(|v| *v == 0.0));
    assert!(rasterize(&[]).is_err());
    assert!(rasterize(&vec![vec![point(0., 0.)]; 129]).is_err());
    let mut strokes = vec![vec![point(1., 2.); 256]; 128];
    assert!(rasterize(&strokes).is_ok());
    strokes[0].push(point(0., 0.));
    assert!(rasterize(&strokes).is_err());
    assert!(rasterize(&[vec![point(f32::NAN, 1.)]]).is_err());
    assert!(normalize(&GrayImage::from_pixel(4, 4, image::Luma([255]))).is_err());
}
#[test]
fn preview_is_inverse_of_the_exact_encoder_tensor_including_padding() {
    let prepared = PreprocessedInk::new(&one_plus_one()).unwrap();
    let preview = prepared.preview();
    let tensor = prepared.into_tensor().unwrap();
    let (shape, pixels) = tensor.try_extract_tensor::<f32>().unwrap();
    assert_eq!(shape.as_ref(), [1, 1, 448, 448]);
    assert_eq!(preview.size, [448, 448]);
    assert_eq!(preview.gray.len(), pixels.len());
    for (gray, value) in preview.gray.iter().zip(pixels) {
        assert!((*gray as f32 / 255.0 - (value * STD + MEAN)).abs() <= 0.5 / 255.0 + 1e-6);
    }
    assert!(preview.gray.contains(&0));
    assert!(preview.gray.contains(&255));
    assert!(pixels[447 * 448..].iter().all(|v| *v == 0.0));
    assert!(preview.gray[447 * 448..].iter().all(|v| *v == 243));
    let vertical = PreprocessedInk::new(&[vec![point(10., 0.), point(10., 100.)]]).unwrap();
    let preview = vertical.preview();
    assert!((0..SIDE).all(|y| vertical.pixels[y * SIDE + SIDE - 1] == 0.0));
    assert!((0..SIDE).all(|y| preview.gray[y * SIDE + SIDE - 1] == 243));
}

#[test]
fn log_probability_is_real_and_stable() {
    let (id, lp) = greedy(&[1000., 1000., 999.]).unwrap();
    assert_eq!(id, 0);
    assert!((lp + (2.0 + (-1f64).exp()).ln()).abs() < 1e-12);
    assert!(greedy(&[f32::NAN]).is_err());
}
#[test]
fn missing_model_is_error() {
    assert!(NeuralRecognizer::load(Path::new("missing-texteller-model-directory")).is_err());
}
#[test]
#[ignore = "需要下载真实 TexTeller 权重；设置 TEXTELLER_MODEL_DIR 后显式运行"]
fn actual_model_one_plus_one() {
    let dir = std::env::var_os("TEXTELLER_MODEL_DIR").expect("设置 TEXTELLER_MODEL_DIR");
    let mut recognizer = NeuralRecognizer::load(Path::new(&dir)).expect("加载真实 ONNX 模型");
    println!(
        "encoder inputs: {:?}; outputs: {:?}",
        recognizer.encoder.inputs, recognizer.encoder.outputs
    );
    println!(
        "decoder inputs: {:?}; logits: {:?}",
        recognizer.decoder.inputs,
        recognizer
            .decoder
            .outputs
            .iter()
            .find(|v| v.name == "logits")
    );
    println!(
        "start={}, eos={}, vocab={}",
        recognizer.start_id, recognizer.eos_id, recognizer.vocab_size
    );
    let strokes = one_plus_one();
    let image = rasterize(&strokes).unwrap();
    let png = recognizer.model_dir().join("actual-one-plus-one.png");
    image.save(&png).unwrap();
    let loaded = image::open(&png).unwrap().to_luma8();
    assert_eq!(normalize(&loaded).unwrap(), normalize(&image).unwrap());
    let start = Instant::now();
    let result = recognizer.recognize(&strokes).expect("真实模型推理");
    println!(
        "PNG={}; elapsed={:?}; raw={:?}",
        png.display(),
        start.elapsed(),
        result
    );
    assert!(result.finished, "模型未在预算内生成 EOS");
    assert!(result.mean_log_probability.is_finite());
    // 不要求模型猜中预设答案；原始结果必须如实保留，不能注入 1+1。
    assert!(!result.latex.is_empty());
    {
        let _slot = INFERENCE_SLOT.lock().unwrap();
        assert!(
            recognizer
                .recognize(&strokes)
                .unwrap_err()
                .contains("识别槽")
        );
    }
    let again = recognizer
        .recognize(&strokes)
        .expect("复用同一会话再次推理");
    println!("reused-session raw={again:?}");
    assert_eq!(result.latex, again.latex);
    assert_eq!(result.generated_tokens, again.generated_tokens);
    assert!(again.finished);
}
