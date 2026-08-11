use super::*;

#[test]
fn percentile_uses_the_nearest_rank() {
    let mut histogram = [0; 11];
    for count in histogram.iter_mut().skip(1) {
        *count = 1;
    }
    assert_eq!(percentile(&histogram, 0.50), 5);
    assert_eq!(percentile(&histogram, 0.95), 10);
}
