mod calendar;
mod config;
mod mqtt;
mod theme;
mod toolguard;

use std::sync::Mutex;

use bevy::prelude::*;

use calendar::CalendarData;
use config::KioskConfig;
use kiosk::KioskDisplayMode;
use mqtt::MqttEvent;
use theme::ThemeColors;
use toolguard::{status_color, status_label, ToolGuardData};

// ── State ─────────────────────────────────────────────────────────────────────

#[derive(States, Default, Clone, Eq, PartialEq, Hash, Debug)]
enum KioskView {
    #[default]
    ToolGuard,
    Calendar,
    HorizontalPanes,
    VerticalPanes,
}

// ── Resources ─────────────────────────────────────────────────────────────────

#[derive(Resource)]
struct MqttReceiver(Mutex<std::sync::mpsc::Receiver<MqttEvent>>);

#[derive(Resource, Default)]
struct ConnectionStatus {
    connected: bool,
    last_error: Option<String>,
    generation: u32,
}

impl ConnectionStatus {
    fn bump(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }
}

#[derive(Resource)]
struct RotationTimer(Timer);

#[derive(Resource)]
struct HasCalendar(bool);

#[derive(Resource)]
struct DisplayMode(KioskDisplayMode);

#[derive(Resource)]
struct Theme(ThemeColors);

// ── Marker components ─────────────────────────────────────────────────────────

#[derive(Component)]
struct ViewRoot;

// ── Fixed semantic colors ─────────────────────────────────────────────────────

const DOT_CONNECTED: Color = Color::srgb(0.196, 0.784, 0.314);
const DOT_DISCONNECTED: Color = Color::srgb(0.784, 0.235, 0.235);

// ── Entry point ───────────────────────────────────────────────────────────────

fn main() {
    let config = KioskConfig::load_or_default();
    let has_calendar = config.calendar_topic.is_some();

    let rx = mqtt::spawn_mqtt_thread(
        config.mqtt_broker_url.clone(),
        config.mqtt_client_id.clone(),
        config.toolguard_topic.clone(),
        config.calendar_topic.clone(),
    );

    let initial_view = match &config.display_mode {
        KioskDisplayMode::HorizontalPanes => KioskView::HorizontalPanes,
        KioskDisplayMode::VerticalPanes => KioskView::VerticalPanes,
        _ => KioskView::ToolGuard,
    };

    let mut app = App::new();

    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Kiosk".to_string(),
            ..default()
        }),
        ..default()
    }))
    .insert_state(initial_view)
    .insert_resource(MqttReceiver(Mutex::new(rx)))
    .insert_resource(ToolGuardData::default())
    .insert_resource(CalendarData::default())
    .insert_resource(ConnectionStatus::default())
    .insert_resource(HasCalendar(has_calendar))
    .insert_resource(DisplayMode(config.display_mode.clone()))
    .insert_resource(Theme(theme::resolve(&config.theme)))
    .add_systems(Startup, setup_camera)
    // ── ToolGuard view ──
    .add_systems(OnEnter(KioskView::ToolGuard), spawn_toolguard_view)
    .add_systems(OnExit(KioskView::ToolGuard), despawn_view)
    .add_systems(
        Update,
        update_toolguard_view
            .run_if(in_state(KioskView::ToolGuard))
            .after(poll_mqtt),
    )
    // ── Calendar view ──
    .add_systems(OnEnter(KioskView::Calendar), spawn_calendar_view)
    .add_systems(OnExit(KioskView::Calendar), despawn_view)
    .add_systems(
        Update,
        update_calendar_view
            .run_if(in_state(KioskView::Calendar))
            .after(poll_mqtt),
    )
    // ── Panes views ──
    .add_systems(
        OnEnter(KioskView::HorizontalPanes),
        spawn_horizontal_panes_view,
    )
    .add_systems(OnExit(KioskView::HorizontalPanes), despawn_view)
    .add_systems(OnEnter(KioskView::VerticalPanes), spawn_vertical_panes_view)
    .add_systems(OnExit(KioskView::VerticalPanes), despawn_view)
    .add_systems(
        Update,
        update_panes_view
            .run_if(in_state(KioskView::HorizontalPanes).or(in_state(KioskView::VerticalPanes)))
            .after(poll_mqtt),
    )
    // ── Shared update ──
    .add_systems(Update, (poll_mqtt, handle_input));

    if let KioskDisplayMode::Automatic { s } = &config.display_mode {
        if has_calendar {
            app.insert_resource(RotationTimer(Timer::from_seconds(
                *s as f32,
                TimerMode::Repeating,
            )));
            app.add_systems(
                Update,
                rotate_views
                    .run_if(resource_exists::<RotationTimer>)
                    .after(poll_mqtt),
            );
        }
    }

    app.run();
}

// ── Setup ─────────────────────────────────────────────────────────────────────

fn setup_camera(mut commands: Commands) {
    commands.spawn(Camera2d);
}

// ── MQTT polling ──────────────────────────────────────────────────────────────

fn poll_mqtt(
    rx: Res<MqttReceiver>,
    mut tg: ResMut<ToolGuardData>,
    mut cal: ResMut<CalendarData>,
    mut conn: ResMut<ConnectionStatus>,
) {
    let Ok(guard) = rx.0.lock() else { return };
    while let Ok(event) = guard.try_recv() {
        match event {
            MqttEvent::Connected => {
                conn.connected = true;
                conn.last_error = None;
                conn.bump();
            }
            MqttEvent::Disconnected => {
                conn.connected = false;
                conn.bump();
            }
            MqttEvent::ToolGuardState(payload) => {
                tg.update(payload);
            }
            MqttEvent::CalendarEvents(events) => {
                cal.update(events);
            }
            MqttEvent::Error(e) => {
                conn.last_error = Some(e);
                conn.bump();
            }
        }
    }
}

// ── Input / rotation ──────────────────────────────────────────────────────────

fn handle_input(
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<State<KioskView>>,
    mut next: ResMut<NextState<KioskView>>,
    has_calendar: Res<HasCalendar>,
    display_mode: Res<DisplayMode>,
) {
    if keys.just_pressed(KeyCode::Space) {
        match &display_mode.0 {
            KioskDisplayMode::Static | KioskDisplayMode::Automatic { .. } => {
                if has_calendar.0 {
                    next.set(match state.get() {
                        KioskView::ToolGuard => KioskView::Calendar,
                        KioskView::Calendar => KioskView::ToolGuard,
                        v => v.clone(),
                    });
                }
            }
            KioskDisplayMode::HorizontalPanes | KioskDisplayMode::VerticalPanes => {}
        }
    }
}

fn rotate_views(
    mut timer: ResMut<RotationTimer>,
    time: Res<Time>,
    state: Res<State<KioskView>>,
    mut next: ResMut<NextState<KioskView>>,
) {
    if timer.0.tick(time.delta()).just_finished() {
        next.set(match state.get() {
            KioskView::ToolGuard => KioskView::Calendar,
            KioskView::Calendar => KioskView::ToolGuard,
            v => v.clone(),
        });
    }
}

// ── View lifecycle ────────────────────────────────────────────────────────────

fn despawn_view(mut commands: Commands, q: Query<Entity, With<ViewRoot>>) {
    for e in &q {
        commands.entity(e).despawn();
    }
}

// ── ToolGuard view ────────────────────────────────────────────────────────────

fn spawn_toolguard_view(
    mut commands: Commands,
    tg: Res<ToolGuardData>,
    conn: Res<ConnectionStatus>,
    theme: Res<Theme>,
) {
    build_toolguard_ui(&mut commands, &tg, &conn, &theme.0);
}

fn update_toolguard_view(
    mut commands: Commands,
    tg: Res<ToolGuardData>,
    conn: Res<ConnectionStatus>,
    theme: Res<Theme>,
    q: Query<Entity, With<ViewRoot>>,
) {
    if !tg.is_changed() && !conn.is_changed() {
        return;
    }
    for e in &q {
        commands.entity(e).despawn();
    }
    build_toolguard_ui(&mut commands, &tg, &conn, &theme.0);
}

fn build_toolguard_ui(
    commands: &mut Commands,
    tg: &ToolGuardData,
    conn: &ConnectionStatus,
    theme: &ThemeColors,
) {
    let root = commands
        .spawn((
            ViewRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme.bg),
        ))
        .id();
    add_toolguard_content(commands, root, tg, conn, theme);
}

// ── Calendar view ─────────────────────────────────────────────────────────────

fn spawn_calendar_view(
    mut commands: Commands,
    cal: Res<CalendarData>,
    conn: Res<ConnectionStatus>,
    theme: Res<Theme>,
) {
    build_calendar_ui(&mut commands, &cal, &conn, &theme.0);
}

fn update_calendar_view(
    mut commands: Commands,
    cal: Res<CalendarData>,
    conn: Res<ConnectionStatus>,
    theme: Res<Theme>,
    q: Query<Entity, With<ViewRoot>>,
) {
    if !cal.is_changed() && !conn.is_changed() {
        return;
    }
    for e in &q {
        commands.entity(e).despawn();
    }
    build_calendar_ui(&mut commands, &cal, &conn, &theme.0);
}

fn build_calendar_ui(
    commands: &mut Commands,
    cal: &CalendarData,
    conn: &ConnectionStatus,
    theme: &ThemeColors,
) {
    let root = commands
        .spawn((
            ViewRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme.bg),
        ))
        .id();
    add_calendar_content(commands, root, cal, conn, theme);
}

// ── Panes views ───────────────────────────────────────────────────────────────

fn spawn_horizontal_panes_view(
    mut commands: Commands,
    tg: Res<ToolGuardData>,
    cal: Res<CalendarData>,
    conn: Res<ConnectionStatus>,
    theme: Res<Theme>,
) {
    build_panes_ui(
        &mut commands,
        &tg,
        &cal,
        &conn,
        &theme.0,
        FlexDirection::Row,
    );
}

fn spawn_vertical_panes_view(
    mut commands: Commands,
    tg: Res<ToolGuardData>,
    cal: Res<CalendarData>,
    conn: Res<ConnectionStatus>,
    theme: Res<Theme>,
) {
    build_panes_ui(
        &mut commands,
        &tg,
        &cal,
        &conn,
        &theme.0,
        FlexDirection::Column,
    );
}

fn update_panes_view(
    mut commands: Commands,
    tg: Res<ToolGuardData>,
    cal: Res<CalendarData>,
    conn: Res<ConnectionStatus>,
    theme: Res<Theme>,
    q: Query<Entity, With<ViewRoot>>,
    state: Res<State<KioskView>>,
) {
    if !tg.is_changed() && !cal.is_changed() && !conn.is_changed() {
        return;
    }
    for e in &q {
        commands.entity(e).despawn();
    }
    let flex_dir = if *state.get() == KioskView::HorizontalPanes {
        FlexDirection::Row
    } else {
        FlexDirection::Column
    };
    build_panes_ui(&mut commands, &tg, &cal, &conn, &theme.0, flex_dir);
}

fn build_panes_ui(
    commands: &mut Commands,
    tg: &ToolGuardData,
    cal: &CalendarData,
    conn: &ConnectionStatus,
    theme: &ThemeColors,
    flex_dir: FlexDirection,
) {
    let (pane_w, pane_h, sep_w, sep_h) = if flex_dir == FlexDirection::Row {
        (
            Val::Percent(50.0),
            Val::Percent(100.0),
            Val::Px(1.0),
            Val::Percent(100.0),
        )
    } else {
        (
            Val::Percent(100.0),
            Val::Percent(50.0),
            Val::Percent(100.0),
            Val::Px(1.0),
        )
    };

    let mut tg_pane = Entity::PLACEHOLDER;
    let mut cal_pane = Entity::PLACEHOLDER;

    commands
        .spawn((
            ViewRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                flex_direction: flex_dir,
                ..default()
            },
            BackgroundColor(theme.bg),
        ))
        .with_children(|outer| {
            tg_pane = outer
                .spawn(Node {
                    width: pane_w,
                    height: pane_h,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip_y(),
                    ..default()
                })
                .id();
            outer.spawn((
                Node {
                    width: sep_w,
                    height: sep_h,
                    flex_shrink: 0.0,
                    ..default()
                },
                BackgroundColor(theme.divider),
            ));
            cal_pane = outer
                .spawn(Node {
                    width: pane_w,
                    height: pane_h,
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::clip_y(),
                    ..default()
                })
                .id();
        });

    add_toolguard_content(commands, tg_pane, tg, conn, theme);
    add_calendar_content(commands, cal_pane, cal, conn, theme);
}

// ── Content builders ──────────────────────────────────────────────────────────

fn add_toolguard_content(
    commands: &mut Commands,
    parent: Entity,
    tg: &ToolGuardData,
    conn: &ConnectionStatus,
    theme: &ThemeColors,
) {
    commands.entity(parent).with_children(|p| {
        // ── Header ──
        p.spawn(Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            padding: UiRect::all(Val::Px(16.0)),
            width: Val::Percent(100.0),
            ..default()
        })
        .with_children(|header| {
            header.spawn((
                Text::new("TOOL STATUS"),
                TextFont {
                    font_size: 28.0,
                    ..default()
                },
                TextColor(theme.content),
            ));
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|ind| {
                    let dot_color = if conn.connected {
                        DOT_CONNECTED
                    } else {
                        DOT_DISCONNECTED
                    };
                    ind.spawn((
                        Node {
                            width: Val::Px(12.0),
                            height: Val::Px(12.0),
                            flex_shrink: 0.0,
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(dot_color),
                    ));
                    ind.spawn((
                        Text::new(if conn.connected {
                            "connected"
                        } else {
                            "disconnected"
                        }),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(theme.dim),
                    ));
                });
        });

        // ── Divider ──
        p.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(theme.divider),
        ));

        // ── Error banner ──
        if let Some(ref err) = conn.last_error {
            p.spawn(Node {
                padding: UiRect::all(Val::Px(12.0)),
                ..default()
            })
            .with_children(|ep| {
                ep.spawn((
                    Text::new(format!("⚠ {err}")),
                    TextFont {
                        font_size: 14.0,
                        ..default()
                    },
                    TextColor(Color::srgb(1.0, 0.4, 0.4)),
                ));
            });
        }

        // ── Tool grid ──
        p.spawn(Node {
            display: Display::Grid,
            grid_template_columns: vec![RepeatedGridTrack::flex(3, 1.0)],
            grid_auto_rows: vec![GridTrack::auto()],
            column_gap: Val::Px(16.0),
            row_gap: Val::Px(16.0),
            padding: UiRect::all(Val::Px(16.0)),
            align_content: AlignContent::FlexStart,
            width: Val::Percent(100.0),
            overflow: Overflow::clip_y(),
            flex_grow: 1.0,
            ..default()
        })
        .with_children(|grid| {
            if tg.tools.is_empty() {
                grid.spawn((
                    Text::new("Waiting for state…"),
                    TextFont {
                        font_size: 36.0,
                        ..default()
                    },
                    TextColor(theme.dim),
                ));
                return;
            }

            for tool in &tg.tools {
                let color = status_color(&tool.status);
                grid.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        border_radius: BorderRadius::all(Val::Px(8.0)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(theme.card_bg),
                ))
                .with_children(|card| {
                    // Colored top strip
                    card.spawn((
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(10.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                    // Text content
                    card.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(16.0)),
                        row_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|info| {
                        info.spawn((
                            Text::new(tool.name.clone()),
                            TextFont {
                                font_size: 44.0,
                                ..default()
                            },
                            TextColor(theme.content),
                        ));
                        info.spawn((
                            Text::new(status_label(&tool.status)),
                            TextFont {
                                font_size: 32.0,
                                ..default()
                            },
                            TextColor(color),
                        ));
                        if let Some(ref user) = tool.inuse_by {
                            info.spawn((
                                Text::new(user.clone()),
                                TextFont {
                                    font_size: 28.0,
                                    ..default()
                                },
                                TextColor(theme.dim),
                            ));
                        }
                    });
                });
            }
        });
    });
}

fn add_calendar_content(
    commands: &mut Commands,
    parent: Entity,
    cal: &CalendarData,
    conn: &ConnectionStatus,
    theme: &ThemeColors,
) {
    commands.entity(parent).with_children(|p| {
        // ── Header ──
        p.spawn(Node {
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            padding: UiRect::all(Val::Px(16.0)),
            width: Val::Percent(100.0),
            ..default()
        })
        .with_children(|header| {
            header.spawn((
                Text::new("UPCOMING EVENTS"),
                TextFont {
                    font_size: 28.0,
                    ..default()
                },
                TextColor(theme.content),
            ));
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(8.0),
                    ..default()
                })
                .with_children(|ind| {
                    let dot_color = if conn.connected {
                        DOT_CONNECTED
                    } else {
                        DOT_DISCONNECTED
                    };
                    ind.spawn((
                        Node {
                            width: Val::Px(12.0),
                            height: Val::Px(12.0),
                            flex_shrink: 0.0,
                            border_radius: BorderRadius::all(Val::Px(6.0)),
                            ..default()
                        },
                        BackgroundColor(dot_color),
                    ));
                    ind.spawn((
                        Text::new(if conn.connected {
                            "connected"
                        } else {
                            "disconnected"
                        }),
                        TextFont {
                            font_size: 14.0,
                            ..default()
                        },
                        TextColor(theme.dim),
                    ));
                });
        });

        // ── Divider ──
        p.spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(1.0),
                ..default()
            },
            BackgroundColor(theme.divider),
        ));

        // ── Event list ──
        p.spawn(Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(16.0)),
            row_gap: Val::Px(14.0),
            overflow: Overflow::clip_y(),
            flex_grow: 1.0,
            ..default()
        })
        .with_children(|list| {
            if cal.events.is_empty() {
                list.spawn((
                    Text::new("No upcoming events"),
                    TextFont {
                        font_size: 18.0,
                        ..default()
                    },
                    TextColor(theme.dim),
                ));
                return;
            }

            for event in &cal.events {
                let color = event.display_color();
                let srgba = color.to_srgba();
                let row_bg = if event.is_ongoing {
                    Color::srgba(srgba.red, srgba.green, srgba.blue, 0.08)
                } else {
                    Color::NONE
                };

                list.spawn((
                    Node {
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::FlexStart,
                        column_gap: Val::Px(16.0),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                        width: Val::Percent(100.0),
                        border_radius: BorderRadius::all(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(row_bg),
                ))
                .with_children(|row| {
                    row.spawn(Node {
                        flex_direction: FlexDirection::Column,
                        width: Val::Px(110.0),
                        flex_shrink: 0.0,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|t| {
                        t.spawn((
                            Text::new(event.date_str()),
                            TextFont {
                                font_size: 18.0,
                                ..default()
                            },
                            TextColor(theme.dim),
                        ));
                        t.spawn((
                            Text::new(event.time_str()),
                            TextFont {
                                font_size: 18.0,
                                ..default()
                            },
                            TextColor(theme.dim),
                        ));
                    });
                    row.spawn((
                        Text::new(event.summary.clone()),
                        TextFont {
                            font_size: 18.0,
                            ..default()
                        },
                        TextColor(color),
                    ));
                });
            }
        });
    });
}
