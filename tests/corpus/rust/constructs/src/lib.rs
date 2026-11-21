//! Constructs with known complexity, nesting, and names.

use std::fmt;

pub fn plain(x: u32) -> u32 {
    x + 1
}

pub fn branches(a: Option<u32>, b: bool, c: bool) -> u32 {
    if let Some(x) = a
        && x > 1
        && b
    {
        return x;
    } else if c || b {
        return 2;
    } else {
        return 3;
    }
}

pub fn matching(v: u32) -> u32 {
    match v {
        0 => 10,
        1 | 2 if v > 1 => 20,
        3 => 30,
        _ => 0,
    }
}

pub fn guarded_default(v: u32) -> u32 {
    match v {
        0 => 1,
        _ if v > 5 => 2,
        _ => 3,
    }
}

pub fn fallible(a: &str) -> Result<u32, std::num::ParseIntError> {
    let n = a.parse::<u32>()?;
    let m = a.trim().parse::<u32>()?;
    Ok(n + m)
}

pub fn let_else(a: Option<u32>) -> u32 {
    let Some(x) = a else {
        return 0;
    };
    x
}

pub fn loops(items: &[u32]) -> u32 {
    let mut total = 0;
    for item in items {
        while total < *item {
            loop {
                total += 1;
                if total > 100 {
                    break;
                }
            }
        }
    }
    total
}

pub fn with_closures(items: &[u32]) -> u32 {
    let doubled: Vec<u32> = items.iter().map(|x| x * 2).collect();
    let evens = doubled.iter().filter(|x| {
        let keep = **x % 4 == 0;
        keep && **x > 0
    });
    let nested = |a: u32| {
        let inner = |b: u32| b + a;
        inner(a)
    };
    evens.count() as u32 + nested(1)
}

pub fn outer() -> u32 {
    fn inner(x: u32) -> u32 {
        if x > 1 { x } else { 1 }
    }
    inner(2)
}

pub struct Wrapper<T> {
    value: T,
}

impl<T: Copy> Wrapper<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }

    pub fn get(&self) -> T {
        self.value
    }
}

impl<T: fmt::Display> fmt::Display for Wrapper<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.value)
    }
}

pub trait Shape {
    fn area(&self) -> f64;

    fn describe(&self) -> String {
        if self.area() > 10.0 {
            "big".to_owned()
        } else {
            "small".to_owned()
        }
    }
}

pub mod geometry {
    pub mod circle {
        pub fn radius(r: f64) -> f64 {
            if r < 0.0 { 0.0 } else { r }
        }
    }

    impl super::Wrapper<f64> {
        pub fn double(&self) -> f64 {
            self.value * 2.0
        }
    }
}

pub const HANDLER: fn(u32) -> u32 = |x| x + 1;

pub fn generic<T: Clone + Default>(x: Option<T>) -> T {
    x.unwrap_or_default()
}

macro_rules! twice {
    ($e:expr) => {
        if $e { 1 } else { 2 }
    };
}
