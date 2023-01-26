use std::{collections::HashSet, io::Write, sync::Arc};

use byteorder::{BigEndian, WriteBytesExt};
pub use components::{
    game_objects::player_camera, player::{prev_raw_input, raw_input}
};
use elements_audio::AudioListener;
use elements_core::{camera::active_camera, main_scene, on_frame, runtime};
use elements_ecs::{query, query_mut, SystemGroup, World};
use elements_element::{element_component, Element, Hooks};
use elements_input::{
    on_app_focus_change, on_app_keyboard_input, on_app_mouse_input, on_app_mouse_motion, on_app_mouse_wheel, ElementState, MouseButton, MouseScrollDelta
};
use elements_network::{
    client::game_client, get_player_by_user_id, player::{local_user_id, user_id}, DatagramHandlers
};
use elements_std::unwrap_log_err;
use elements_ui::VirtualKeyCode;
use elements_world_audio::audio_listener;
use glam::{Mat4, Vec2, Vec3};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};

const PLAYER_INPUT_DATAGRAM_ID: u32 = 5;

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct RawInput {
    pub keys: HashSet<VirtualKeyCode>,
    pub mouse_position: Vec2,
    pub mouse_wheel: f32,
    pub mouse_buttons: HashSet<MouseButton>,
}

mod components {
    pub mod game_objects {
        use elements_ecs::components;

        components!("game_objects", {
            // attached to a camera entity to mark it as belonging to a player
            // player is located through user_id
            player_camera: (),
        });
    }

    pub mod player {
        use elements_ecs::components;

        use super::super::RawInput;

        components!("player", {
            raw_input: RawInput,
            prev_raw_input: RawInput,
        });
    }
}

pub fn init_all_components() {
    components::game_objects::init_components();
    components::player::init_components();
}

pub fn register_datagram_handler(handlers: &mut DatagramHandlers) {
    handlers.insert(
        PLAYER_INPUT_DATAGRAM_ID,
        Arc::new(|state, _assets, user_id, data| {
            let input: RawInput = unwrap_log_err!(bincode::deserialize(&data));
            let mut state = state.lock();
            if let Some(world) = state.get_player_world_mut(user_id) {
                if let Some(player_id) = get_player_by_user_id(world, user_id) {
                    world.set(player_id, raw_input(), input).ok();
                }
            }
        }),
    );
}

pub fn server_systems_final() -> SystemGroup {
    SystemGroup::new(
        "player/server_systems_final",
        vec![query_mut(prev_raw_input(), raw_input()).to_system(|q, world, qs, _| {
            for (_, prev, input) in q.iter(world, qs) {
                *prev = input.clone();
            }
        })],
    )
}

pub fn client_systems() -> SystemGroup {
    SystemGroup::new(
        "player/client_systems",
        vec![query((player_camera(), user_id())).spawned().to_system(|q, world, qs, _| {
            // TEMP: This synchronises server cameras to the client. This is a temporary solution until this
            // is moved to/controlled by clientside scripting.

            let local = world.resource(local_user_id()).clone();
            for (id, (_, player_id)) in q.collect_cloned(world, qs) {
                if player_id == local {
                    world.add_component(id, active_camera(), 0.).unwrap();
                    world.add_component(id, main_scene(), ()).unwrap();

                    log::info!("Adding audio listener to: {id} {player_id:?}");
                    // Add a listener on the local camera for each client
                    world
                        .add_component(id, audio_listener(), Arc::new(Mutex::new(AudioListener::new(Mat4::IDENTITY, Vec3::X * 0.2))))
                        .unwrap();
                }
            }
        })],
    )
}

#[element_component]
pub fn PlayerRawInputHandler(_: &mut World, hooks: &mut Hooks) -> Element {
    const PIXELS_PER_LINE: f32 = 5.0;

    let input = hooks.use_ref_with(RawInput::default);
    let (has_focus, set_has_focus) = hooks.use_state(false);

    Element::new()
        .listener(
            on_app_focus_change(),
            Arc::new(move |_, _, focus| {
                set_has_focus(focus);
            }),
        )
        .listener(
            on_app_keyboard_input(),
            Arc::new({
                let input = input.clone();
                move |_, _, event| {
                    if let Some(keycode) = event.keycode {
                        let mut lock = input.lock();
                        match event.state {
                            ElementState::Pressed => {
                                lock.keys.insert(keycode);
                            }
                            ElementState::Released => {
                                lock.keys.remove(&keycode);
                            }
                        }
                    }
                    true
                }
            }),
        )
        .listener(
            on_app_mouse_motion(),
            Arc::new({
                let input = input.clone();
                move |_, _, delta| {
                    input.lock().mouse_position += delta;
                }
            }),
        )
        .listener(
            on_app_mouse_wheel(),
            Arc::new({
                let input = input.clone();
                move |_, _, delta| {
                    input.lock().mouse_wheel += match delta {
                        MouseScrollDelta::LineDelta(_, y) => y * PIXELS_PER_LINE,
                        MouseScrollDelta::PixelDelta(pos) => pos.y as f32,
                    };
                    true
                }
            }),
        )
        .listener(
            on_app_mouse_input(),
            Arc::new({
                let input = input.clone();
                move |_, _, event| {
                    let mut lock = input.lock();
                    match event.state {
                        ElementState::Pressed => {
                            lock.mouse_buttons.insert(event.button);
                        }
                        ElementState::Released => {
                            lock.mouse_buttons.remove(&event.button);
                        }
                    }
                }
            }),
        )
        .listener(
            on_frame(),
            Arc::new(move |world, _, _| {
                if !has_focus {
                    return;
                }

                if let Some(Some(gc)) = world.resource_opt(game_client()).cloned() {
                    let runtime = world.resource(runtime()).clone();
                    let input = input.clone();

                    runtime.spawn(async move {
                        let mut data = Vec::new();
                        data.write_u32::<BigEndian>(PLAYER_INPUT_DATAGRAM_ID).unwrap();

                        let msg = bincode::serialize(&*input.lock()).unwrap();
                        data.write_all(&msg).unwrap();
                        gc.connection.send_datagram(data.into()).ok();
                    });
                }
            }),
        )
}