use std::{cell::RefCell, rc::Rc};

use stereokit_rust::{
    event_loop::{IStepper, StepperId},
    font::Font,
    material::Material,
    maths::{Matrix, Quat, Vec3},
    mesh::Mesh,
    sk::{MainThreadToken, SkInfo},
    system::{Text, TextStyle},
    util::named_colors::RED,
};

pub struct AStepper {
    id: StepperId,
    sk_info: Option<Rc<RefCell<SkInfo>>>,
    round_cube: Mesh,
    pub transform: Matrix,
    pub text: String,
    material: Material,
    text_style: Option<TextStyle>,
}

unsafe impl Send for AStepper {}

impl Default for AStepper {
    fn default() -> Self {
        Self {
            id: "AStepper".to_string(),
            sk_info: None,
            round_cube: Mesh::generate_rounded_cube(Vec3::ONE / 5.0, 0.2, Some(16)),
            transform: Matrix::tr(&((Vec3::NEG_Z * 2.5) + Vec3::Y), &Quat::from_angles(0.0, 180.0, 0.0)),
            text: "Stepper A".to_owned(),
            material: Material::pbr().copy(),
            text_style: Some(Text::make_style(Font::default(), 0.3, RED)),
        }
    }
}

impl IStepper for AStepper {
    fn initialize(&mut self, id: StepperId, sk_info: Rc<RefCell<SkInfo>>) -> bool {
        self.id = id;
        self.sk_info = Some(sk_info);

        true
    }

    fn step(&mut self, token: &MainThreadToken) {
        self.round_cube.draw(token, &self.material, self.transform, Some(RED.into()), None);
        Text::add_at(token, &self.text, self.transform, self.text_style, None, None, None, None, None, None);
    }

    fn shutdown(&mut self) {}
}
