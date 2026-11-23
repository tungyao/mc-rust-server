//! 确定性 2D/3D value noise 与 hash（无第三方依赖）。
//!
//! 同一 (seed, 坐标) 永远得到同一结果。负坐标先转 `i64` 再转 `u64`，
//! 依靠 wrapping 乘法实现，不依赖 `as` 的符号语义。2D 版本保持 P14 的
//! lattice 公式不变，因此 `height_at` 的历史输出（出生 y 等）不漂移。

/// quintic 插值权重（C2 连续，避免网格线）。
pub(crate) fn smootherstep(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

pub(crate) fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

/// splitmix64 收尾混合。
pub(crate) fn mix64(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

const LATTICE_X: u64 = 0x9E37_79B9_7F4A_7C15;
const LATTICE_Y: u64 = 0xD6E8_FEB8_6659_FD93;
const LATTICE_Z: u64 = 0xC2B2_AE3D_27D4_EB4F;

/// 整数格点 hash → `[0,1)`；`y=0` 时与 P14 的 2D lattice 完全一致。
pub(crate) fn hash01(seed: i64, x: i64, y: i64, z: i64, salt: u64) -> f64 {
    let mut h = (x as u64).wrapping_mul(LATTICE_X)
        ^ (y as u64).wrapping_mul(LATTICE_Y)
        ^ (z as u64).wrapping_mul(LATTICE_Z)
        ^ (seed as u64)
        ^ salt;
    h = mix64(h);
    // 取高 53 位，保证 f64 精确可表示。
    ((h >> 11) as f64) * (1.0 / ((1u64 << 53) as f64))
}

/// 2D 平滑 value noise，输出 `[0,1)`。
pub(crate) fn value_noise_2d(seed: i64, x: f64, z: f64, scale: f64, salt: u64) -> f64 {
    let fx = x / scale;
    let fz = z / scale;
    let x0f = fx.floor();
    let z0f = fz.floor();
    let tx = smootherstep(fx - x0f);
    let tz = smootherstep(fz - z0f);
    let x0 = x0f as i64;
    let z0 = z0f as i64;
    let v00 = hash01(seed, x0, 0, z0, salt);
    let v10 = hash01(seed, x0 + 1, 0, z0, salt);
    let v01 = hash01(seed, x0, 0, z0 + 1, salt);
    let v11 = hash01(seed, x0 + 1, 0, z0 + 1, salt);
    lerp(lerp(v00, v10, tx), lerp(v01, v11, tx), tz)
}

/// 3D 平滑 value noise（三线性），输出 `[0,1)`；用于洞穴。
pub(crate) fn value_noise_3d(seed: i64, x: f64, y: f64, z: f64, scale: f64, salt: u64) -> f64 {
    let fx = x / scale;
    let fy = y / scale;
    let fz = z / scale;
    let x0f = fx.floor();
    let y0f = fy.floor();
    let z0f = fz.floor();
    let tx = smootherstep(fx - x0f);
    let ty = smootherstep(fy - y0f);
    let tz = smootherstep(fz - z0f);
    let x0 = x0f as i64;
    let y0 = y0f as i64;
    let z0 = z0f as i64;
    let v000 = hash01(seed, x0, y0, z0, salt);
    let v100 = hash01(seed, x0 + 1, y0, z0, salt);
    let v010 = hash01(seed, x0, y0 + 1, z0, salt);
    let v110 = hash01(seed, x0 + 1, y0 + 1, z0, salt);
    let v001 = hash01(seed, x0, y0, z0 + 1, salt);
    let v101 = hash01(seed, x0 + 1, y0, z0 + 1, salt);
    let v011 = hash01(seed, x0, y0 + 1, z0 + 1, salt);
    let v111 = hash01(seed, x0 + 1, y0 + 1, z0 + 1, salt);
    let a = lerp(lerp(v000, v100, tx), lerp(v010, v110, tx), ty);
    let b = lerp(lerp(v001, v101, tx), lerp(v011, v111, tx), ty);
    lerp(a, b, tz)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_noise_are_deterministic_and_bounded() {
        for &(x, y, z) in &[(0, 0, 0), (-1, 5, 7), (1_000_000, -40, -900_000)] {
            let a = hash01(42, x, y, z, 7);
            let b = hash01(42, x, y, z, 7);
            assert_eq!(a, b);
            assert!((0.0..1.0).contains(&a));
            assert_ne!(hash01(42, x, y, z, 7), hash01(43, x, y, z, 7));
        }
        for i in 0..200 {
            let x = (i as f64) * 7.3 - 400.0;
            let z = 13.0 - (i as f64) * 3.1;
            let n = value_noise_2d(1, x, z, 32.0, 9);
            assert!((0.0..1.0).contains(&n), "2d noise {n} at {x},{z}");
            let n3 = value_noise_3d(1, x, z / 2.0, z, 24.0, 9);
            assert!((0.0..1.0).contains(&n3), "3d noise {n3} at {x},{z}");
        }
    }

    #[test]
    fn y_zero_keeps_the_p14_2d_lattice() {
        // y=0 项是 0，XOR 不改变 P14 的 2D 公式。
        let h = hash01(123456789, 369, 0, 381, 0x9E37_79B9_7F4A_7C15);
        let mut raw = (369u64).wrapping_mul(LATTICE_X)
            ^ (381u64).wrapping_mul(LATTICE_Z)
            ^ (123456789u64)
            ^ 0x9E37_79B9_7F4A_7C15;
        raw = mix64(raw);
        assert_eq!(h, ((raw >> 11) as f64) * (1.0 / ((1u64 << 53) as f64)));
    }

    #[test]
    fn value_noise_is_smooth_along_axes() {
        let mut prev = value_noise_2d(7, 0.0, 3.0, 24.0, 1);
        for i in 1..400 {
            let v = value_noise_2d(7, i as f64 * 0.25, 3.0, 24.0, 1);
            assert!((v - prev).abs() < 0.12, "2d noise jump at {i}: {prev}->{v}");
            prev = v;
        }
        let mut prev = value_noise_3d(7, 1.0, 0.0, 2.0, 24.0, 1);
        for i in 1..400 {
            let v = value_noise_3d(7, 1.0, i as f64 * 0.25, 2.0, 24.0, 1);
            assert!((v - prev).abs() < 0.12, "3d noise jump at {i}: {prev}->{v}");
            prev = v;
        }
    }
}
