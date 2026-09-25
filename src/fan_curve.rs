//! 温度曲线风扇控制。
//!
//! 曲线格式（环境变量 OMEN_FAN_CURVE）：
//!   "50:30,65:50,80:80,90:100"
//!
//! 规则：
//!   - 点按温度升序，解析时自动排序
//!   - 低于第一个点 → floor（最低档）
//!   - 高于最后一个点 → ceiling（最高档）
//!   - 两点之间 → 线性插值

use crate::error::OmenError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Point {
    pub temp: u8,
    pub speed: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FanCurve {
    points: Vec<Point>,
}

impl FanCurve {
    pub fn new(mut points: Vec<Point>) -> Result<Self, OmenError> {
        if points.is_empty() {
            return Err(OmenError::Unsupported);
        }
        for p in &points {
            if p.speed > 100 {
                return Err(OmenError::OutOfRange {
                    value: p.speed as u32,
                    min: 0,
                    max: 100,
                });
            }
        }
        points.sort_by_key(|p| p.temp);
        Ok(Self { points })
    }

    /// 从 "50:30,65:50,80:80,90:100" 解析。
    pub fn parse(s: &str) -> Result<Self, OmenError> {
        let mut points = Vec::new();
        for part in s.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            let (t, sp) = part
                .split_once(':')
                .ok_or(OmenError::Unsupported)?;
            points.push(Point {
                temp: t.trim().parse().map_err(|_| OmenError::Unsupported)?,
                speed: sp.trim().parse().map_err(|_| OmenError::Unsupported)?,
            });
        }
        Self::new(points)
    }

    /// 给定温度，返回目标风扇转速百分比。
    pub fn evaluate(&self, temp: u8) -> u8 {
        let pts = &self.points;
        let last = pts.len() - 1;

        if temp <= pts[0].temp {
            return pts[0].speed;
        }
        if temp >= pts[last].temp {
            return pts[last].speed;
        }

        for w in pts.windows(2) {
            let (a, b) = (&w[0], &w[1]);
            if temp >= a.temp && temp <= b.temp {
                let ratio = (temp - a.temp) as f32 / (b.temp - a.temp) as f32;
                let speed = a.speed as f32 + ratio * (b.speed - a.speed) as f32;
                return speed.round() as u8;
            }
        }
        pts[last].speed
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve() -> FanCurve {
        FanCurve::parse("50:30,65:50,80:80,90:100").unwrap()
    }

    #[test]
    fn parse_basic() {
        let c = curve();
        assert_eq!(c.points().len(), 4);
        assert_eq!(c.points()[0], Point { temp: 50, speed: 30 });
    }

    #[test]
    fn parse_unsorted_becomes_sorted() {
        let c = FanCurve::parse("90:100,50:30,80:80,65:50").unwrap();
        assert_eq!(c.points()[0].temp, 50);
        assert_eq!(c.points()[3].temp, 90);
    }

    #[test]
    fn evaluate_below_floor() {
        assert_eq!(curve().evaluate(40), 30);
    }

    #[test]
    fn evaluate_above_ceiling() {
        assert_eq!(curve().evaluate(95), 100);
    }

    #[test]
    fn evaluate_exact_point() {
        assert_eq!(curve().evaluate(65), 50);
    }

    #[test]
    fn evaluate_between_points() {
        let c = curve();
        assert_eq!(c.evaluate(57), 39);
        assert_eq!(c.evaluate(72), 64);
    }

    #[test]
    fn evaluate_single_point() {
        let c = FanCurve::parse("60:50").unwrap();
        assert_eq!(c.evaluate(40), 50);
        assert_eq!(c.evaluate(60), 50);
        assert_eq!(c.evaluate(80), 50);
    }

    #[test]
    fn parse_empty_fails() {
        assert!(FanCurve::parse("").is_err());
    }

    #[test]
    fn parse_bad_format_fails() {
        assert!(FanCurve::parse("50-30").is_err());
        assert!(FanCurve::parse("abc:def").is_err());
    }

    #[test]
    fn speed_over_100_fails() {
        assert!(FanCurve::parse("50:150").is_err());
    }
}
