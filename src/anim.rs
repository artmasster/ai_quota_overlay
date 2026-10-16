//! Small CSS-like transitions: each key eases toward its target over a fixed duration.
use std::collections::HashMap;
use std::hash::Hash;

pub struct Fades<K> {
    cur: HashMap<K, (f32, f32)>,
    secs: f32,
}

impl<K: Copy + Eq + Hash> Fades<K> {
    pub fn new(secs: f32) -> Self {
        Self { cur: HashMap::new(), secs }
    }

    /// Sets which keys are "on" (target 1); every other key heads to 0.
    pub fn target(&mut self, on: &[K]) {
        for (k, v) in self.cur.iter_mut() {
            v.1 = if on.contains(k) { 1.0 } else { 0.0 };
        }
        for k in on {
            self.cur.entry(*k).or_insert((0.0, 1.0)).1 = 1.0;
        }
    }

    /// Advances by `dt` seconds; true while anything is still moving.
    pub fn step(&mut self, dt: f32) -> bool {
        let d = if self.secs <= 0.0 { 1.0 } else { dt / self.secs };
        let mut moving = false;
        self.cur.retain(|_, (v, t)| {
            if *v < *t {
                *v = (*v + d).min(*t);
            } else if *v > *t {
                *v = (*v - d).max(*t);
            }
            if *v != *t {
                moving = true;
            }
            !(*v == 0.0 && *t == 0.0)
        });
        moving
    }

    /// Eased value (CSS `ease`) of a key.
    pub fn get(&self, k: K) -> f32 {
        self.cur.get(&k).map(|(v, _)| crate::ui::ease(*v)).unwrap_or(0.0)
    }

    pub fn map(&self) -> HashMap<K, f32> {
        self.cur.keys().map(|k| (*k, self.get(*k))).collect()
    }

    pub fn snap(&mut self) {
        self.cur.retain(|_, (v, t)| {
            *v = *t;
            *t != 0.0
        });
    }
}
