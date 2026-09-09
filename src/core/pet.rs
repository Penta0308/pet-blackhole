use std::time::{Duration, Instant};

use crate::platform::window::WindowCommand;

const NODE_COUNT: usize = 24;
const REST_CENTER: glam::Vec2 = glam::Vec2::new(0.5, 0.5);
const REST_RADIUS: glam::Vec2 = glam::Vec2::new(0.145, 0.128);

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct TickInput {
    pub dt: f32,
    pub window_pos: (f64, f64),
    pub window_size: (f64, f64),
    pub monitor: Rect,
    pub dragging: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct PetRenderSnapshot {
    pub time: f32,
    pub opacity: f32,
    pub points: [[f32; 2]; NODE_COUNT],
    pub wobble: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PetMode {
    Idle,
    Dragging,
    Wandering,
    Sleeping,
}

pub struct PetState {
    mode: PetMode,
    age: f32,
    idle_for: Duration,
    last_activity: Instant,
    velocity: glam::Vec2,
    wander_target: glam::Vec2,
    edge_pressure: [f32; 4],
    nodes: [glam::Vec2; NODE_COUNT],
    node_velocity: [glam::Vec2; NODE_COUNT],
    last_window_pos: Option<glam::Vec2>,
    drag_slosh: glam::Vec2,
    pending_window_commands: Vec<WindowCommand>,
}

impl Default for PetState {
    fn default() -> Self {
        let nodes = std::array::from_fn(rest_node);
        Self {
            mode: PetMode::Idle,
            age: 0.0,
            idle_for: Duration::ZERO,
            last_activity: Instant::now(),
            velocity: glam::Vec2::ZERO,
            wander_target: glam::Vec2::new(220.0, 160.0),
            edge_pressure: [0.0; 4],
            nodes,
            node_velocity: [glam::Vec2::ZERO; NODE_COUNT],
            last_window_pos: None,
            drag_slosh: glam::Vec2::ZERO,
            pending_window_commands: Vec::new(),
        }
    }
}

impl PetState {
    pub fn note_user_activity(&mut self) {
        self.last_activity = Instant::now();
        if self.mode != PetMode::Dragging {
            self.mode = PetMode::Idle;
        }
    }

    pub fn set_dragging(&mut self, dragging: bool, _cursor: (f32, f32)) {
        if dragging {
            self.mode = PetMode::Dragging;
        } else {
            self.mode = PetMode::Idle;
            self.note_user_activity();
        }
    }

    pub fn tick(&mut self, input: TickInput) {
        self.pending_window_commands.clear();
        self.age += input.dt;
        self.idle_for = Instant::now().saturating_duration_since(self.last_activity);

        if input.dragging {
            self.mode = PetMode::Dragging;
        } else if self.idle_for > Duration::from_secs(300) {
            self.mode = PetMode::Sleeping;
        } else if self.idle_for > Duration::from_secs(45) {
            self.mode = PetMode::Wandering;
        } else if self.mode != PetMode::Dragging {
            self.mode = PetMode::Idle;
        }

        self.update_drag_slosh(&input);
        self.update_edge_pressure(&input);
        self.step_soft_body(input.dt);

        if self.mode == PetMode::Wandering {
            self.update_wander(input);
        }
    }

    pub fn next_frame_delay(&self) -> Duration {
        match self.mode {
            PetMode::Dragging => Duration::from_millis(16),
            PetMode::Wandering => Duration::from_millis(33),
            PetMode::Idle => Duration::from_millis(90),
            PetMode::Sleeping => Duration::from_millis(250),
        }
    }

    pub fn render_snapshot(&self) -> PetRenderSnapshot {
        PetRenderSnapshot {
            time: self.age,
            opacity: 0.78,
            points: self.nodes.map(|p| [p.x, p.y]),
            wobble: match self.mode {
                PetMode::Dragging => 1.0,
                PetMode::Wandering => 0.7,
                PetMode::Idle => 0.25,
                PetMode::Sleeping => 0.08,
            },
        }
    }

    pub fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        std::mem::take(&mut self.pending_window_commands)
    }

    pub fn hit_test(&self, cursor: (f32, f32), window_size: (f64, f64)) -> bool {
        let w = window_size.0.max(1.0) as f32;
        let h = window_size.1.max(1.0) as f32;
        let p = glam::Vec2::new(cursor.0 / w, cursor.1 / h);
        let center = self.nodes.iter().copied().sum::<glam::Vec2>() / NODE_COUNT as f32;
        let dir_vec = p - center;
        let dist = dir_vec.length();
        let dir = if dist > 0.0001 {
            dir_vec / dist
        } else {
            glam::Vec2::X
        };

        let mut radius_sum = 0.0;
        let mut weight_sum = 0.0;
        for node in self.nodes {
            let q = node - center;
            let q_len = q.length().max(0.0001);
            let q_dir = q / q_len;
            let weight = dir.dot(q_dir).max(0.0).powf(18.0) + 0.0002;
            radius_sum += q_len * weight;
            weight_sum += weight;
        }

        dist <= radius_sum / weight_sum + 0.08
    }

    fn update_drag_slosh(&mut self, input: &TickInput) {
        let pos = glam::Vec2::new(input.window_pos.0 as f32, input.window_pos.1 as f32);
        let velocity = self
            .last_window_pos
            .map(|last| (pos - last) / input.dt.max(0.001))
            .unwrap_or(glam::Vec2::ZERO);
        self.last_window_pos = Some(pos);

        let target = if input.dragging {
            (-velocity / 900.0).clamp_length_max(0.20)
        } else {
            glam::Vec2::ZERO
        };
        let blend = 1.0 - (-(if input.dragging { 18.0 } else { 5.0 }) * input.dt).exp();
        self.drag_slosh += (target - self.drag_slosh) * blend;
    }

    fn update_edge_pressure(&mut self, input: &TickInput) {
        let threshold = 42.0_f64;
        let w = input.window_size.0.max(1.0);
        let h = input.window_size.1.max(1.0);
        let (min_x, max_x, min_y, max_y) = self.node_bounds();
        let body_left = input.window_pos.0 + f64::from(min_x) * w;
        let body_right = input.window_pos.0 + f64::from(max_x) * w;
        let body_top = input.window_pos.1 + f64::from(min_y) * h;
        let body_bottom = input.window_pos.1 + f64::from(max_y) * h;

        let left = body_left - input.monitor.x;
        let top = body_top - input.monitor.y;
        let right = input.monitor.x + input.monitor.width - body_right;
        let bottom = input.monitor.y + input.monitor.height - body_bottom;

        let target = [
            pressure(left, threshold) as f32,
            pressure(right, threshold) as f32,
            pressure(top, threshold) as f32,
            pressure(bottom, threshold) as f32,
        ];

        let stiffness = if input.dragging { 14.0 } else { 5.0 };
        let blend = 1.0 - (-stiffness * input.dt).exp();
        for (current, target) in self.edge_pressure.iter_mut().zip(target) {
            *current += (target - *current) * blend;
            if *current < 0.001 {
                *current = 0.0;
            }
        }
    }

    fn step_soft_body(&mut self, dt: f32) {
        let dt = dt.clamp(0.001, 0.033);
        let [left, right, top, bottom] = self.edge_pressure;
        let contact = left.max(right).max(top).max(bottom);
        let k_shape = 95.0;
        let k_edge = 135.0;
        let k_spring = 48.0;
        let damping = (0.80_f32).powf(dt * 60.0);

        let old_nodes = self.nodes;
        let mean = old_nodes.iter().copied().sum::<glam::Vec2>() / NODE_COUNT as f32;
        let center_target =
            REST_CENTER + glam::Vec2::new((left - right) * 0.085, (top - bottom) * 0.085);

        for i in 0..NODE_COUNT {
            let side = side_weights(i);
            let horizontal_contact = left.max(right);
            let vertical_contact = top.max(bottom);
            let rest = rest_node(i);
            let relative = rest - REST_CENTER;
            let anisotropic = glam::Vec2::new(
                relative.x * (1.0 - horizontal_contact * 0.36 + vertical_contact * 0.16),
                relative.y * (1.0 - vertical_contact * 0.36 + horizontal_contact * 0.22),
            );
            let slosh_strength = if self.mode == PetMode::Dragging {
                1.0
            } else {
                0.35
            };
            let mut target =
                REST_CENTER + anisotropic + (center_target - REST_CENTER) * (0.35 + contact * 0.65);
            target += self.drag_slosh
                * slosh_strength
                * (0.35 + side.dot(-self.drag_slosh.normalize_or_zero()).max(0.0));

            if left > 0.0 && side.x < -0.05 {
                target.x = lerp(target.x, 0.185, left * (-side.x).min(1.0));
                target.y += (target.y - 0.5) * left * 0.08;
            }
            if right > 0.0 && side.x > 0.05 {
                target.x = lerp(target.x, 0.815, right * side.x.min(1.0));
                target.y += (target.y - 0.5) * right * 0.08;
            }
            if top > 0.0 && side.y < -0.05 {
                target.y = lerp(target.y, 0.185, top * (-side.y).min(1.0));
                target.x += (target.x - 0.5) * top * 0.08;
            }
            if bottom > 0.0 && side.y > 0.05 {
                target.y = lerp(target.y, 0.815, bottom * side.y.min(1.0));
                target.x += (target.x - 0.5) * bottom * 0.08;
            }

            let prev = old_nodes[(i + NODE_COUNT - 1) % NODE_COUNT];
            let next = old_nodes[(i + 1) % NODE_COUNT];
            let rest_prev = rest_node((i + NODE_COUNT - 1) % NODE_COUNT);
            let rest_next = rest_node((i + 1) % NODE_COUNT);
            let spring_force = spring(old_nodes[i], prev, rest_node(i).distance(rest_prev))
                + spring(old_nodes[i], next, rest_node(i).distance(rest_next));
            let area_force = (mean - old_nodes[i]) * contact * 12.0;
            let force = (target - old_nodes[i]) * (k_shape + contact * k_edge)
                + spring_force * k_spring
                + area_force;

            self.node_velocity[i] = (self.node_velocity[i] + force * dt) * damping;
            self.nodes[i] += self.node_velocity[i] * dt;
            self.nodes[i] = self.nodes[i].clamp(glam::Vec2::splat(0.035), glam::Vec2::splat(0.965));
        }
    }

    fn node_bounds(&self) -> (f32, f32, f32, f32) {
        let mut min_x = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for node in self.nodes {
            min_x = min_x.min(node.x);
            max_x = max_x.max(node.x);
            min_y = min_y.min(node.y);
            max_y = max_y.max(node.y);
        }
        (min_x, max_x, min_y, max_y)
    }

    fn update_wander(&mut self, input: TickInput) {
        let current = glam::Vec2::new(input.window_pos.0 as f32, input.window_pos.1 as f32);
        let target_distance = current.distance(self.wander_target);
        if target_distance < 18.0 {
            let t = self.age;
            let max_x = (input.monitor.width - input.window_size.0).max(1.0) as f32;
            let max_y = (input.monitor.height - input.window_size.1).max(1.0) as f32;
            self.wander_target = glam::Vec2::new(
                input.monitor.x as f32 + pseudo01(t * 1.37) * max_x,
                input.monitor.y as f32 + pseudo01(t * 2.11 + 7.0) * max_y,
            );
        }

        let desired = (self.wander_target - current).clamp_length_max(26.0);
        self.velocity = self.velocity.lerp(desired, (input.dt * 0.8).min(1.0));
        let next = current + self.velocity * input.dt;
        self.pending_window_commands.push(WindowCommand::MoveTo {
            x: f64::from(next.x),
            y: f64::from(next.y),
        });
    }
}

fn rest_node(i: usize) -> glam::Vec2 {
    let a = i as f32 / NODE_COUNT as f32 * std::f32::consts::TAU;
    REST_CENTER + glam::Vec2::new(a.cos() * REST_RADIUS.x, a.sin() * REST_RADIUS.y)
}

fn side_weights(i: usize) -> glam::Vec2 {
    let a = i as f32 / NODE_COUNT as f32 * std::f32::consts::TAU;
    glam::Vec2::new(a.cos(), a.sin())
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t.clamp(0.0, 1.0)
}

fn spring(node: glam::Vec2, neighbor: glam::Vec2, rest_length: f32) -> glam::Vec2 {
    let delta = neighbor - node;
    let length = delta.length().max(0.001);
    delta / length * (length - rest_length)
}

fn pressure(distance: f64, threshold: f64) -> f64 {
    if distance >= threshold {
        0.0
    } else {
        let linear = (1.0 - (distance.max(0.0) / threshold)).clamp(0.0, 1.0);
        linear.powf(0.70)
    }
}

fn pseudo01(x: f32) -> f32 {
    (x.sin() * 43_758.547).fract().abs()
}
