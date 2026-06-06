use anyhow::{Result, anyhow};

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> Result<f32> {
    if a.is_empty() || b.is_empty() {
        return Err(anyhow!("Vectors cannot be empty"));
    }
    if a.len() != b.len() {
        return Err(anyhow!(
            "Dimension mismatch: {} vs {}",
            a.len(),
            b.len()
        ));
    }

    let mut dot_product = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;

    for (x, y) in a.iter().zip(b.iter()) {
        dot_product += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }

    let norm_a = norm_a.sqrt();
    let norm_b = norm_b.sqrt();

    if norm_a.is_nan() || norm_b.is_nan() {
        return Ok(0.0);
    }

    if norm_a < 1e-10 || norm_b < 1e-10 {
        return Ok(0.0);
    }

    let similarity = dot_product / (norm_a * norm_b);
    if similarity.is_nan() {
        return Ok(0.0);
    }
    Ok(similarity.clamp(-1.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_basic() -> Result<()> {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&a, &b)?;
        assert!((sim - 1.0).abs() < 1e-6);

        let c = vec![0.0, 1.0, 0.0];
        let sim2 = cosine_similarity(&a, &c)?;
        assert!((sim2 - 0.0).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn test_cosine_similarity_empty() {
        let a = vec![];
        let b = vec![1.0];
        assert!(cosine_similarity(&a, &b).is_err());
        assert!(cosine_similarity(&b, &a).is_err());
    }

    #[test]
    fn test_cosine_similarity_mismatch() {
        let a = vec![1.0, 2.0];
        let b = vec![1.0, 2.0, 3.0];
        assert!(cosine_similarity(&a, &b).is_err());
    }

    #[test]
    fn test_cosine_similarity_zero_magnitude() -> Result<()> {
        let a = vec![0.0, 0.0];
        let b = vec![1.0, 2.0];
        let sim = cosine_similarity(&a, &b)?;
        assert_eq!(sim, 0.0);
        Ok(())
    }

    #[test]
    fn test_cosine_similarity_clamping() -> Result<()> {
        let a = vec![1.0, 1.0];
        let b = vec![1.0, 1.0];
        let sim = cosine_similarity(&a, &b)?;
        assert!(sim >= -1.0 && sim <= 1.0);
        Ok(())
    }

    #[test]
    fn test_cosine_similarity_orthogonal() -> Result<()> {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        let sim = cosine_similarity(&a, &b)?;
        assert!((sim - 0.0).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn test_cosine_similarity_opposite() -> Result<()> {
        let a = vec![1.0, 2.0];
        let b = vec![-1.0, -2.0];
        let sim = cosine_similarity(&a, &b)?;
        assert!((sim - (-1.0)).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn test_cosine_similarity_near_zero_magnitude() -> Result<()> {
        // Norm of a is sqrt(2e-24) ≈ 1.414e-12, which is < 1e-10.
        // Should return 0.0 because of the norm safety threshold.
        let a = vec![1e-12, 1e-12];
        let b = vec![1.0, 1.0];
        let sim = cosine_similarity(&a, &b)?;
        assert_eq!(sim, 0.0);
        Ok(())
    }

    #[test]
    fn test_cosine_similarity_negative_dot_product() -> Result<()> {
        let a = vec![1.0, -1.0];
        let b = vec![-1.0, 1.0];
        let sim = cosine_similarity(&a, &b)?;
        assert!((sim - (-1.0)).abs() < 1e-6);
        Ok(())
    }

    #[test]
    fn test_cosine_similarity_nan_handling() -> Result<()> {
        let a = vec![std::f32::NAN, 1.0];
        let b = vec![1.0, 2.0];
        let sim = cosine_similarity(&a, &b)?;
        assert_eq!(sim, 0.0);
        Ok(())
    }

    #[test]
    fn test_cosine_similarity_stress_fuzz() -> Result<()> {
        let special_values = [
            0.0,
            -0.0,
            1.0,
            -1.0,
            1e-45,     // extremely small f32
            1e38,      // extremely large f32
            std::f32::INFINITY,
            std::f32::NEG_INFINITY,
            std::f32::NAN,
        ];

        for &x1 in &special_values {
            for &x2 in &special_values {
                for &y1 in &special_values {
                    for &y2 in &special_values {
                        let a = vec![x1, x2];
                        let b = vec![y1, y2];
                        let res = cosine_similarity(&a, &b);
                        match res {
                            Ok(sim) => {
                                assert!(!sim.is_nan(), "Similarity must not be NaN for inputs {:?}, {:?}", a, b);
                                assert!(sim >= -1.0 && sim <= 1.0, "Similarity {} out of bounds for inputs {:?}, {:?}", sim, a, b);
                            }
                            Err(_) => {}
                        }
                    }
                }
            }
        }
        Ok(())
    }
}
