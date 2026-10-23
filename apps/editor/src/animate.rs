//! Animation in the editor: keyframe CSS for previews, and the timeline panel that edits a
//! shape's animation.

use leptos::prelude::*;
use vellum_schema::{css, Animation, Drawing, Easing, Keyframe, Node, NodeKind};

/// The `@keyframes` name the canvas uses for the element with this id. `generation` changes
/// the name so a replay restarts every animation.
pub fn canvas_name(id: &str, generation: u32) -> String {
    format!("vl-{}-{generation}", css::slug(id))
}

/// One `@keyframes` rule as CSS text.
pub fn keyframes_css(name: &str, animation: &Animation) -> String {
    let stops: String = animation
        .keyframe_rules()
        .into_iter()
        .map(|(stop, declarations)| {
            let body: Vec<_> = declarations
                .iter()
                .map(|(p, v)| format!("{p}:{v}"))
                .collect();
            format!("{stop}{{{}}}", body.join(";"))
        })
        .collect();
    format!("@keyframes {name}{{{stops}}}")
}

/// Keyframes for every animated node and shape in `nodes`, and in the components they place.
pub fn page_keyframes(doc: &vellum_schema::Document, nodes: &[Node], generation: u32) -> String {
    fn walk(
        doc: &vellum_schema::Document,
        nodes: &[Node],
        generation: u32,
        depth: u32,
        out: &mut String,
    ) {
        for n in nodes {
            if let Some(a) = &n.animation {
                out.push_str(&keyframes_css(&canvas_name(&n.id, generation), a));
            }
            match &n.kind {
                NodeKind::Svg { drawing } => out.push_str(&drawing_keyframes(drawing, generation)),
                NodeKind::Container { children, .. } => walk(doc, children, generation, depth, out),
                NodeKind::Instance { component, .. } if depth < 8 => {
                    if let Some(c) = doc.component(component) {
                        walk(doc, &c.nodes, generation, depth + 1, out);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = String::new();
    walk(doc, nodes, generation, 0, &mut out);
    out
}

pub fn drawing_keyframes(drawing: &Drawing, generation: u32) -> String {
    drawing
        .shapes
        .iter()
        .filter_map(|s| {
            let a = s.animation.as_ref()?;
            Some(keyframes_css(&canvas_name(&s.id, generation), a))
        })
        .collect()
}

/// The inline style that plays `animation` on the element with `id`.
pub fn play_style(id: &str, animation: &Animation, generation: u32, svg_shape: bool) -> String {
    animation
        .declarations(&canvas_name(id, generation), svg_shape)
        .into_iter()
        .map(|(p, v)| format!("{p}:{v}"))
        .collect::<Vec<_>>()
        .join(";")
}

/// The inline style that freezes `animation` at `percent` of its first run, for scrubbing.
pub fn scrub_style(id: &str, animation: &Animation, generation: u32, percent: f64) -> String {
    let duration = animation.duration.max(0.01);
    format!(
        "animation:{} {}s {} -{}s 1 normal both paused;transform-box:fill-box;transform-origin:center",
        canvas_name(id, generation),
        css::number(duration),
        animation.easing.as_css(),
        css::number(duration * percent.clamp(0.0, 100.0) / 100.0),
    )
}

/// Edits the animation of shape `id` in `drawing`: presets, timing, and a timeline of
/// keyframes with a scrubber.
pub fn shape_animation_panel(
    drawing: RwSignal<Drawing>,
    id: String,
    scrub: RwSignal<Option<f64>>,
    replay: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let picked = RwSignal::new(None::<usize>);
    let id_get = id.clone();
    let animation = Memo::new(move |_| {
        drawing.with(|d| {
            d.shapes
                .iter()
                .find(|s| s.id == id_get)
                .and_then(|s| s.animation.clone())
        })
    });
    // Stored so the closures below stay `Copy`.
    let id_set = StoredValue::new(id.clone());
    let update = move |f: &dyn Fn(&mut Option<Animation>)| {
        drawing.update(|d| {
            let id = id_set.get_value();
            if let Some(s) = d.shapes.iter_mut().find(|s| s.id == id) {
                f(&mut s.animation);
            }
        })
    };
    let edit = move |f: &dyn Fn(&mut Animation)| {
        update(&|a| {
            if let Some(a) = a {
                f(a)
            }
        })
    };

    let preset_value = move || match animation.get() {
        None => String::new(),
        Some(a) => Animation::PRESETS
            .iter()
            .find(|(id, _)| Animation::preset(id).as_ref() == Some(&a))
            .map_or("custom".into(), |(id, _)| (*id).to_owned()),
    };
    let on_preset = move |ev| {
        let v = event_target_value(&ev);
        if v == "custom" {
            return;
        }
        update(&|a| *a = Animation::preset(&v));
        picked.set(None);
        if !v.is_empty() {
            replay();
        }
    };
    let presets = Animation::PRESETS
        .iter()
        .map(|&(id, label)| view! { <option value=id>{label}</option> })
        .collect_view();

    let number = move |label: &'static str,
                       min: f64,
                       max: f64,
                       step: f64,
                       get: fn(&Animation) -> f64,
                       set: fn(&mut Animation, f64)| {
        view! {
            <label class="field">
                <span>{label} " (" {move || animation.get().map(|a| css::number(get(&a)))} "s)"</span>
                <input
                    type="range"
                    min=min
                    max=max
                    step=step
                    prop:value=move || animation.get().map(|a| get(&a)).unwrap_or_default()
                    on:input=move |ev| {
                        let v: f64 = event_target_value(&ev).parse().unwrap_or(0.0);
                        edit(&|a| set(a, v));
                    }
                />
            </label>
        }
    };

    let easing = view! {
        <label class="field">
            <span>"Easing"</span>
            <select
                prop:value=move || animation.get().map(|a| a.easing.as_str()).unwrap_or_default()
                on:change=move |ev| {
                    if let Some(e) = Easing::parse(&event_target_value(&ev)) {
                        edit(&|a| a.easing = e);
                    }
                }
            >
                {Easing::ALL.iter().map(|e| view! { <option value=e.as_str()>{e.as_str()}</option> }).collect_view()}
            </select>
        </label>
    };
    let repeat = view! {
        <div class="row">
            <label class="field grow">
                <span>"Repeat"</span>
                <select
                    prop:value=move || animation.get().map(|a| a.iterations.to_string()).unwrap_or_default()
                    on:change=move |ev| {
                        let n: u32 = event_target_value(&ev).parse().unwrap_or(1);
                        edit(&|a| a.iterations = n);
                    }
                >
                    <option value="1">"Once"</option>
                    <option value="2">"Twice"</option>
                    <option value="3">"3 times"</option>
                    <option value="0">"Forever"</option>
                </select>
            </label>
            <label class="check">
                <input
                    type="checkbox"
                    prop:checked=move || animation.get().is_some_and(|a| a.alternate)
                    on:change=move |ev| {
                        let on = event_target_checked(&ev);
                        edit(&|a| a.alternate = on);
                    }
                />
                "Back and forth"
            </label>
        </div>
    };

    // The timeline: a track with a marker per keyframe and a scrubber that previews any point.
    let markers = move || {
        animation.get().map(|a| {
            a.keyframes
                .iter()
                .enumerate()
                .map(|(i, k)| {
                    view! {
                        <button
                            type="button"
                            class="keyframe-marker"
                            class:active=move || picked.get() == Some(i)
                            style=format!("left:{}%", k.at.clamp(0.0, 100.0))
                            title=format!("Keyframe at {}%", css::number(k.at))
                            on:click=move |_| {
                                picked.set(Some(i));
                                let at = animation.get_untracked().and_then(|a| a.keyframes.get(i).map(|k| k.at));
                                scrub.set(at);
                            }
                        ></button>
                    }
                })
                .collect_view()
        })
    };
    let add_keyframe = move |_| {
        let at = scrub.get_untracked().unwrap_or(50.0);
        let mut index = None;
        drawing.update(|d| {
            if let Some(a) = d
                .shapes
                .iter_mut()
                .find(|s| s.id == id)
                .and_then(|s| s.animation.as_mut())
            {
                a.keyframes.push(Keyframe::rest(at));
                a.keyframes.sort_by(|x, y| x.at.total_cmp(&y.at));
                index = a.keyframes.iter().position(|k| k.at == at);
            }
        });
        picked.set(index);
    };
    let remove_keyframe = move |_| {
        if let Some(i) = picked.get_untracked() {
            edit(&|a| {
                if a.keyframes.len() > 2 && i < a.keyframes.len() {
                    a.keyframes.remove(i);
                }
            });
            picked.set(None);
        }
    };

    let key_field = move |label: &'static str,
                          step: f64,
                          get: fn(&Keyframe) -> f64,
                          set: fn(&mut Keyframe, f64)| {
        view! {
            <label class="field key-field">
                <span>{label}</span>
                <input
                    type="number"
                    step=step
                    prop:value=move || {
                        let i = picked.get()?;
                        animation.get().and_then(|a| a.keyframes.get(i).map(|k| css::number(get(k))))
                    }
                    on:input=move |ev| {
                        let Ok(v) = event_target_value(&ev).parse::<f64>() else { return };
                        let Some(i) = picked.get_untracked() else { return };
                        edit(&|a| {
                            if let Some(k) = a.keyframes.get_mut(i) {
                                set(k, v);
                            }
                        });
                        if label == "At %" {
                            scrub.set(Some(v));
                        }
                    }
                />
            </label>
        }
    };
    // Rebuilt only when another keyframe is picked, so typing keeps focus.
    let keyframe_fields = move || {
        picked.get().map(|_| {
            view! {
                <div class="key-grid">
                    {key_field("At %", 1.0, |k| k.at, |k, v| k.at = v.clamp(0.0, 100.0))}
                    {key_field("Opacity", 0.1, |k| k.opacity, |k, v| k.opacity = v.clamp(0.0, 1.0))}
                    {key_field("X", 1.0, |k| k.x, |k, v| k.x = v)}
                    {key_field("Y", 1.0, |k| k.y, |k, v| k.y = v)}
                    {key_field("Scale", 0.05, |k| k.scale, |k, v| k.scale = v)}
                    {key_field("Rotate", 5.0, |k| k.rotate, |k, v| k.rotate = v)}
                </div>
            }
        })
    };

    view! {
        <div class="animation-panel">
            <h3>"Animation"</h3>
            <label class="field">
                <span>"Preset"</span>
                <select prop:value=preset_value on:change=on_preset>
                    <option value="">"None"</option>
                    {presets}
                    <option value="custom" disabled=true>"Custom"</option>
                </select>
            </label>
            // Hidden rather than rebuilt, so inputs keep focus while their values change.
            <div class="fields" class:hidden=move || animation.get().is_none()>
                {number("Duration", 0.1, 10.0, 0.1, |a| a.duration, |a, v| a.duration = v)}
                {number("Delay", 0.0, 5.0, 0.1, |a| a.delay, |a, v| a.delay = v)}
                {easing}
                {repeat}
                <div class="timeline">
                    <div class="timeline-track">{markers}</div>
                    <input
                        type="range"
                        min="0"
                        max="100"
                        step="1"
                        class="scrubber"
                        title="Drag to preview any point"
                        prop:value=move || scrub.get().unwrap_or(0.0)
                        on:input=move |ev| scrub.set(event_target_value(&ev).parse().ok())
                    />
                </div>
                <div class="row">
                    <button type="button" on:click=add_keyframe title="Add a keyframe at the scrubber">"+ Keyframe"</button>
                    <button type="button" disabled=move || picked.get().is_none() on:click=remove_keyframe>"Remove"</button>
                    <button type="button" class="primary" on:click=move |_| { scrub.set(None); replay(); }>"▶ Play"</button>
                </div>
                {keyframe_fields}
            </div>
        </div>
    }
}
