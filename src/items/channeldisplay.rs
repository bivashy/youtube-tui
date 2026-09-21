use super::ItemInfo;
use crate::{
    config::*,
    global::{
        common::channel::{ChannelPlaylists, ChannelVideos},
        functions::*,
        structs::*,
        traits::{Collection, SearchProviderWrapper},
    },
};
use ratatui::{
    layout::{Constraint, Rect},
    style::Style,
    widgets::{Block, Borders},
};
use std::{
    sync::{Mutex, OnceLock},
    thread,
};
use tui_additions::{
    framework::{FrameworkClean, FrameworkItem},
    widgets::{Grid, TextList},
};

const PREFETCH_DISTANCE: usize = 5;

struct PrefetchedVideos {
    page_id: String,
    result: Result<ChannelVideos, String>,
}

struct PrefetchedPlaylists {
    page_id: String,
    result: Result<ChannelPlaylists, String>,
}

#[derive(Default)]
struct ChannelPrefetch {
    videos_in_flight: Option<String>,
    videos_ready: Option<PrefetchedVideos>,
    playlists_in_flight: Option<String>,
    playlists_ready: Option<PrefetchedPlaylists>,
}

static CHANNEL_PREFETCH: OnceLock<Mutex<ChannelPrefetch>> = OnceLock::new();

fn channel_prefetch() -> &'static Mutex<ChannelPrefetch> {
    CHANNEL_PREFETCH.get_or_init(|| Mutex::new(ChannelPrefetch::default()))
}

fn spawn_videos_prefetch(page_id: String, ctoken: String) {
    {
        let mut store = channel_prefetch().lock().unwrap();
        if store.videos_in_flight.as_deref() == Some(page_id.as_str())
            || store
                .videos_ready
                .as_ref()
                .is_some_and(|ready| ready.page_id == page_id)
        {
            return;
        }
        store.videos_in_flight = Some(page_id.clone());
    }

    let provider = SearchProviderWrapper::provider_clone();
    thread::spawn(move || {
        let result = provider
            .channel_videos_continuation(&page_id, &ctoken)
            .map_err(|e| e.to_string());
        let mut store = channel_prefetch().lock().unwrap();
        store.videos_ready = Some(PrefetchedVideos { page_id, result });
        store.videos_in_flight = None;
    });
}

fn take_videos_prefetch(page_id: &str) -> Option<Result<ChannelVideos, String>> {
    let mut store = channel_prefetch().lock().unwrap();
    let ready = store.videos_ready.take()?;
    if ready.page_id == page_id {
        Some(ready.result)
    } else {
        store.videos_ready = Some(ready);
        None
    }
}

fn spawn_playlists_prefetch(page_id: String, ctoken: String) {
    {
        let mut store = channel_prefetch().lock().unwrap();
        if store.playlists_in_flight.as_deref() == Some(page_id.as_str())
            || store
                .playlists_ready
                .as_ref()
                .is_some_and(|ready| ready.page_id == page_id)
        {
            return;
        }
        store.playlists_in_flight = Some(page_id.clone());
    }

    let provider = SearchProviderWrapper::provider_clone();
    thread::spawn(move || {
        let result = provider
            .channel_playlists_continuation(&page_id, &ctoken)
            .map_err(|e| e.to_string());
        let mut store = channel_prefetch().lock().unwrap();
        store.playlists_ready = Some(PrefetchedPlaylists { page_id, result });
        store.playlists_in_flight = None;
    });
}

fn take_playlists_prefetch(page_id: &str) -> Option<Result<ChannelPlaylists, String>> {
    let mut store = channel_prefetch().lock().unwrap();
    let ready = store.playlists_ready.take()?;
    if ready.page_id == page_id {
        Some(ready.result)
    } else {
        store.playlists_ready = Some(ready);
        None
    }
}

/// the 4 pages that a channel has (including the default "blank" page when loading)
#[derive(Clone, Default)]
pub enum ChannelDisplay {
    /// a blank item, will turn into one of the other variants when `.load()` depending on the page
    #[default]
    None,
    /// main channel display page
    Main {
        channel: Box<Item>,
        iteminfo: Box<ItemInfo>,
        grid: Grid,
        textlist: TextList,
        commands: Vec<(String, String)>,
    },
    /// latest videos
    Videos {
        videos: Vec<Item>,
        textlist: TextList,
        iteminfo: Box<ItemInfo>,
        grid: Grid,
        continuation: Option<String>,
    },
    /// created playlists
    Playlists {
        playlists: Vec<Item>,
        textlist: TextList,
        iteminfo: Box<ItemInfo>,
        grid: Grid,
        continuation: Option<String>,
    },
}

impl ChannelDisplay {
    pub fn new_textlist_with_map(commands: Vec<(String, String)>) -> TextList {
        TextList::default()
            .items(
                &commands
                    .iter()
                    .map(|command| &command.0)
                    .collect::<Vec<_>>(),
            )
            .unwrap()
    }

    fn inflate_load(&self, mainconfig: &MainConfig, status: &Status) -> Vec<(String, String)> {
        match self {
            Self::Main { channel, .. } => {
                vec![
                    (
                        String::from("url"),
                        match status.provider {
                            Provider::Invidious => format!(
                                "{}/channel/{}{}",
                                mainconfig.invidious_instance,
                                channel.id().unwrap_or_default(),
                                match self {
                                    Self::None | Self::Main { .. } => "",
                                    Self::Videos { .. } => "videos",
                                    Self::Playlists { .. } => "playlists",
                                }
                            ),
                            Provider::YouTube => {
                                format!("'https://youtu.be/{}'", channel.id().unwrap_or_default())
                            }
                        },
                    ),
                    (
                        String::from("id"),
                        channel.id().unwrap_or_default().to_string(),
                    ),
                    (
                        String::from("name"),
                        channel.fullchannel().unwrap().name.clone(),
                    ),
                ]
            }
            // TODO?
            _ => Vec::new(),
        }
    }

    fn infalte_item_update(
        &self,
        mainconfig: &MainConfig,
        status: &Status,
    ) -> Vec<(String, String)> {
        match self {
            ChannelDisplay::Videos {
                videos, textlist, ..
            } => {
                if textlist.items.is_empty() {
                    vec![(String::from("hover-url"), "no-videos".to_string())]
                } else {
                    vec![(
                        String::from("hover-url"),
                        format!(
                            "{}/watch?v={}",
                            match status.provider {
                                Provider::YouTube => "https://youtube.com",
                                Provider::Invidious => &mainconfig.invidious_instance,
                            },
                            videos[textlist.selected].id().unwrap_or_default()
                        ),
                    )]
                }
            }
            ChannelDisplay::Playlists {
                playlists,
                textlist,
                ..
            } => {
                if textlist.items.is_empty() {
                    vec![(String::from("hover-url"), "no-videos".to_string())]
                } else {
                    vec![(
                        String::from("hover-url"),
                        format!(
                            "{}/playlist?list={}",
                            match status.provider {
                                Provider::YouTube => "https://youtube.com",
                                Provider::Invidious => &mainconfig.invidious_instance,
                            },
                            playlists[textlist.selected].id().unwrap_or_default()
                        ),
                    )]
                }
            }
            _ => Vec::new(),
        }
    }
    /// update the style of the item (colours, etc), ran on ever render
    fn update_appearance(
        &mut self,
        info: &tui_additions::framework::ItemInfo,
        appearance: &AppearanceConfig,
    ) {
        match self {
            ChannelDisplay::Main { textlist, grid, .. }
            | ChannelDisplay::Playlists { textlist, grid, .. }
            | ChannelDisplay::Videos { textlist, grid, .. } => {
                textlist.set_border_type(appearance.borders);
                textlist.set_style(Style::default().fg(appearance.colors.text));

                if info.selected {
                    textlist
                        .set_selected_style(Style::default().fg(appearance.colors.text_special));
                    textlist.set_cursor_style(Style::default().fg(appearance.colors.outline_hover));
                    grid.set_border_style(Style::default().fg(appearance.colors.outline_selected));
                } else {
                    if info.hover {
                        grid.set_border_style(Style::default().fg(appearance.colors.outline_hover));
                    } else {
                        grid.set_border_style(Style::default().fg(appearance.colors.outline));
                    }
                    textlist
                        .set_selected_style(Style::default().fg(appearance.colors.text_secondary));
                    textlist
                        .set_cursor_style(Style::default().fg(appearance.colors.outline_secondary));
                }
            }
            _ => {}
        }
    }

    /// handles when select (enter) is pressed, generally loads the hovered item in a
    /// `SingleItemPage`
    fn select_at_cursor(&self, framework: &mut FrameworkClean) {
        match self {
            Self::None => {}
            Self::Main {
                textlist, commands, ..
            } => {
                let command_string = commands[textlist.selected].1.clone();

                framework
                    .data
                    .state
                    .get_mut::<Tasks>()
                    .unwrap()
                    .priority
                    .push(Task::Command(apply_envs(command_string)));
            }
            Self::Videos {
                videos, textlist, ..
            } => {
                if !videos.is_empty() {
                    framework
                        .data
                        .state
                        .get_mut::<Tasks>()
                        .unwrap()
                        .priority
                        .push(Task::LoadPage(Page::SingleItem(SingleItemPage::Video(
                            videos[textlist.selected].minivideo().unwrap().id.clone(),
                        ))));
                } else {
                    *framework.data.global.get_mut::<Message>().unwrap() =
                        Message::Error(String::from("There is nothing to select"));
                }
            }
            Self::Playlists {
                playlists,
                textlist,
                ..
            } => {
                if !playlists.is_empty() {
                    framework
                        .data
                        .state
                        .get_mut::<Tasks>()
                        .unwrap()
                        .priority
                        .push(Task::LoadPage(Page::SingleItem(SingleItemPage::Playlist(
                            playlists[textlist.selected]
                                .miniplaylist()
                                .unwrap()
                                .id
                                .clone(),
                        ))));
                } else {
                    *framework.data.global.get_mut::<Message>().unwrap() =
                        Message::Error(String::from("There is nothing to select"));
                }
            }
        }
    }

    /// updates the video/playlist preview
    fn update(&mut self) {
        match self {
            Self::Videos {
                videos: items,
                textlist,
                iteminfo,
                ..
            }
            | Self::Playlists {
                playlists: items,
                textlist,
                iteminfo,
                ..
            } => {
                if !items.is_empty()
                    && items[textlist.selected].id() != iteminfo.item.as_ref().unwrap().id()
                {
                    iteminfo.item = Some(items[textlist.selected].clone())
                }
            }
            _ => {}
        }
    }

    fn near_end(&self) -> bool {
        match self {
            Self::Videos {
                videos,
                textlist,
                continuation,
                ..
            } => {
                continuation.is_some()
                    && !videos.is_empty()
                    && textlist.selected + PREFETCH_DISTANCE >= videos.len()
            }
            Self::Playlists {
                playlists,
                textlist,
                continuation,
                ..
            } => {
                continuation.is_some()
                    && !playlists.is_empty()
                    && textlist.selected + PREFETCH_DISTANCE >= playlists.len()
            }
            _ => false,
        }
    }

    fn ensure_prefetch(&self, framework: &mut FrameworkClean) {
        if !self.near_end() {
            return;
        }
        let page_id = framework
            .data
            .state
            .get::<Page>()
            .unwrap()
            .channeldisplay()
            .id
            .clone();

        match self {
            Self::Videos { continuation, .. } => {
                if let Some(ctoken) = continuation.clone() {
                    spawn_videos_prefetch(page_id, ctoken);
                }
            }
            Self::Playlists { continuation, .. } => {
                if let Some(ctoken) = continuation.clone() {
                    spawn_playlists_prefetch(page_id, ctoken);
                }
            }
            _ => {}
        }
    }

    fn apply_prefetch(&mut self, framework: &mut FrameworkClean) -> bool {
        let page_id = framework
            .data
            .state
            .get::<Page>()
            .unwrap()
            .channeldisplay()
            .id
            .clone();
        let (image_index, display_images) = {
            let config = framework.data.global.get::<MainConfig>().unwrap();
            (config.image_index, config.images.display())
        };

        match self {
            Self::Videos {
                videos,
                textlist,
                continuation,
                ..
            } => {
                let Some(result) = take_videos_prefetch(&page_id) else {
                    return false;
                };
                match result {
                    Ok(next) => {
                        *continuation = next.continuation.clone();
                        if !next.videos.is_empty() {
                            let new_items = next
                                .videos
                                .into_iter()
                                .map(|video| Item::from_common_video(video, image_index))
                                .collect::<Vec<_>>();
                            if display_images {
                                download_all_images(
                                    new_items.iter().map(|item| item.into()).collect(),
                                );
                            }
                            videos.extend(new_items);
                            let _ = textlist.set_items(videos.as_slice());
                        }
                        if let Some(ctoken) = continuation.clone() {
                            spawn_videos_prefetch(page_id, ctoken);
                        }
                        true
                    }
                    Err(e) => {
                        *framework.data.global.get_mut::<Message>().unwrap() = Message::Error(e);
                        false
                    }
                }
            }
            Self::Playlists {
                playlists,
                textlist,
                continuation,
                ..
            } => {
                let Some(result) = take_playlists_prefetch(&page_id) else {
                    return false;
                };
                match result {
                    Ok(next) => {
                        *continuation = next.continuation.clone();
                        if !next.playlists.is_empty() {
                            let new_items = next
                                .playlists
                                .into_iter()
                                .map(Item::from_common_playlist)
                                .collect::<Vec<_>>();
                            if display_images {
                                download_all_images(
                                    new_items.iter().map(|item| item.into()).collect(),
                                );
                            }
                            playlists.extend(new_items);
                            let _ = textlist.set_items(playlists.as_slice());
                        }
                        if let Some(ctoken) = continuation.clone() {
                            spawn_playlists_prefetch(page_id, ctoken);
                        }
                        true
                    }
                    Err(e) => {
                        *framework.data.global.get_mut::<Message>().unwrap() = Message::Error(e);
                        false
                    }
                }
            }
            _ => false,
        }
    }

    fn update_pagination(&mut self, framework: &mut FrameworkClean) {
        if !self.near_end() {
            return;
        }
        self.ensure_prefetch(framework);
        self.apply_prefetch(framework);
    }

    /// check if self should be able to be selected
    pub fn selectable(&self) -> bool {
        !matches!(self, Self::None)
    }
}

impl FrameworkItem for ChannelDisplay {
    fn render(
        &mut self,
        frame: &mut ratatui::Frame,
        framework: &mut tui_additions::framework::FrameworkClean,
        area: ratatui::layout::Rect,
        popup_render: bool,
        info: tui_additions::framework::ItemInfo,
    ) {
        if popup_render {
            return;
        }

        let appearance = framework.data.global.get::<AppearanceConfig>().unwrap();
        self.update_appearance(&info, appearance);
        let border_style = Style::default().fg(if info.hover {
            appearance.colors.outline_hover
        } else if info.selected {
            appearance.colors.outline_selected
        } else {
            appearance.colors.outline
        });

        match self {
            Self::None => {
                let block = Block::default()
                    .border_type(appearance.borders)
                    .borders(Borders::ALL)
                    .border_style(border_style);
                frame.render_widget(block, area);
            }
            Self::Main {
                grid,
                textlist,
                iteminfo,
                ..
            } => {
                let chunks = grid.chunks(area).unwrap()[0].clone();
                frame.render_widget(grid.clone(), area);
                iteminfo.render(frame, framework, chunks[0], popup_render, info);
                textlist.set_height(chunks[1].height);
                frame.render_widget(textlist.clone(), chunks[1]);
            }
            Self::Videos {
                textlist,
                iteminfo,
                grid,
                ..
            }
            | Self::Playlists {
                textlist,
                iteminfo,
                grid,
                ..
            } => {
                let inner = &grid.chunks(area).unwrap()[0];

                frame.render_widget(grid.clone(), area);
                textlist.set_height(inner[0].height);
                frame.render_widget(textlist.clone(), inner[0]);
                iteminfo.render(frame, framework, inner[1], popup_render, info);
            }
        }
    }

    fn select(&mut self, _framework: &mut tui_additions::framework::FrameworkClean) -> bool {
        self.selectable()
    }

    fn message(
        &mut self,
        framework: &mut FrameworkClean,
        data: std::collections::HashMap<String, Box<dyn std::any::Any>>,
    ) -> bool {
        if !data.contains_key("type") {
            return false;
        }

        self.update_pagination(framework);

        let updated = match self {
            Self::None => false,
            Self::Main { textlist, .. } => data.get("type").is_some_and(|v| {
                v.downcast_ref::<String>()
                    .is_some_and(|v| match v.as_str() {
                        "scrollup" => textlist.up().is_ok(),
                        "scrolldown" => textlist.down().is_ok(),
                        _ => false,
                    })
            }),
            Self::Videos {
                textlist,
                videos,
                iteminfo,
                ..
            } => data.get("type").is_some_and(|v| {
                let updated = v
                    .downcast_ref::<String>()
                    .is_some_and(|v| match v.as_str() {
                        "scrollup" => textlist.up().is_ok(),
                        "scrolldown" => textlist.down().is_ok(),
                        _ => false,
                    });

                if updated && !videos.is_empty() {
                    framework
                        .data
                        .state
                        .get_mut::<Tasks>()
                        .unwrap()
                        .priority
                        .push(Task::RenderAll);
                    framework
                        .data
                        .global
                        .get_mut::<Status>()
                        .unwrap()
                        .render_image = true;
                    iteminfo.item = Some(videos[textlist.selected].clone());
                }
                updated
            }),
            Self::Playlists {
                playlists,
                textlist,
                iteminfo,
                ..
            } => {
                let updated = data.get("type").is_some_and(|v| {
                    v.downcast_ref::<String>()
                        .is_some_and(|v| match v.as_str() {
                            "scrollup" => textlist.up().is_ok(),
                            "scrolldown" => textlist.down().is_ok(),
                            _ => false,
                        })
                });

                if updated && !playlists.is_empty() {
                    framework
                        .data
                        .state
                        .get_mut::<Tasks>()
                        .unwrap()
                        .priority
                        .push(Task::RenderAll);
                    framework
                        .data
                        .global
                        .get_mut::<Status>()
                        .unwrap()
                        .render_image = true;
                    iteminfo.item = Some(playlists[textlist.selected].clone());
                }

                updated
            }
        };

        set_envs(
            self.infalte_item_update(
                framework.data.global.get::<MainConfig>().unwrap(),
                framework.data.global.get::<Status>().unwrap(),
            )
            .into_iter(),
            &mut framework.data.state.get_mut::<StateEnvs>().unwrap().0,
        );

        updated
    }

    fn key_event(
        &mut self,
        framework: &mut tui_additions::framework::FrameworkClean,
        key: crossterm::event::KeyEvent,
        _info: tui_additions::framework::ItemInfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        self.update_pagination(framework);

        let action = if let Some(action) = framework
            .data
            .global
            .get::<KeyBindingsConfig>()
            .unwrap()
            .get(key)
        {
            action
        } else {
            return Ok(());
        };

        match self {
            Self::Videos {
                videos,
                textlist,
                iteminfo,
                ..
            } => {
                let updated = match action {
                    KeyAction::MoveUp => textlist.up().is_ok(),
                    KeyAction::MoveDown => textlist.down().is_ok(),
                    KeyAction::MoveLeft | KeyAction::First => textlist.first().is_ok(),
                    KeyAction::MoveRight | KeyAction::End => {
                        !videos.is_empty() && textlist.last().is_ok()
                    }
                    KeyAction::Select => {
                        self.select_at_cursor(framework);
                        return Ok(());
                    }
                    _ => false,
                };

                if updated && !videos.is_empty() {
                    framework
                        .data
                        .state
                        .get_mut::<Tasks>()
                        .unwrap()
                        .priority
                        .push(Task::RenderAll);
                    framework
                        .data
                        .global
                        .get_mut::<Status>()
                        .unwrap()
                        .render_image = true;
                    iteminfo.item = Some(videos[textlist.selected].clone());
                    set_envs(
                        self.infalte_item_update(
                            framework.data.global.get::<MainConfig>().unwrap(),
                            framework.data.global.get::<Status>().unwrap(),
                        )
                        .into_iter(),
                        &mut framework.data.state.get_mut::<StateEnvs>().unwrap().0,
                    );
                }
            }
            Self::Playlists {
                playlists,
                textlist,
                iteminfo,
                ..
            } => {
                let updated = match action {
                    KeyAction::MoveUp => textlist.up().is_ok(),
                    KeyAction::MoveDown => textlist.down().is_ok(),
                    KeyAction::MoveLeft => textlist.first().is_ok(),
                    KeyAction::MoveRight => !playlists.is_empty() && textlist.last().is_ok(),
                    KeyAction::Select => {
                        self.select_at_cursor(framework);
                        return Ok(());
                    }
                    _ => false,
                };

                if updated && !playlists.is_empty() {
                    framework
                        .data
                        .state
                        .get_mut::<Tasks>()
                        .unwrap()
                        .priority
                        .push(Task::RenderAll);
                    framework
                        .data
                        .global
                        .get_mut::<Status>()
                        .unwrap()
                        .render_image = true;
                    iteminfo.item = Some(playlists[textlist.selected].clone());
                }
            }
            Self::Main {
                textlist, commands, ..
            } => {
                let updated = match action {
                    KeyAction::MoveUp => textlist.up().is_ok(),
                    KeyAction::MoveDown => textlist.down().is_ok(),
                    KeyAction::MoveLeft | KeyAction::First => textlist.first().is_ok(),
                    KeyAction::MoveRight | KeyAction::End => textlist.last().is_ok(),
                    KeyAction::Select => {
                        self.select_at_cursor(framework);
                        framework
                            .data
                            .state
                            .get_mut::<Tasks>()
                            .unwrap()
                            .priority
                            .push(Task::RenderAll);
                        return Ok(());
                    }
                    _ => false,
                };

                if updated && !commands.is_empty() {
                    framework
                        .data
                        .state
                        .get_mut::<Tasks>()
                        .unwrap()
                        .priority
                        .push(Task::RenderAll);
                    framework
                        .data
                        .global
                        .get_mut::<Status>()
                        .unwrap()
                        .render_image = true;
                }
            }
            _ => {}
        }

        Ok(())
    }

    fn load_item(
        &mut self,
        framework: &mut tui_additions::framework::FrameworkClean,
        _info: tui_additions::framework::ItemInfo,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mainconfig = framework.data.global.get::<MainConfig>().unwrap();
        let appearance = framework.data.global.get::<AppearanceConfig>().unwrap();
        let page = framework.data.state.get::<Page>().unwrap().channeldisplay();
        let page_id = page.id.clone();

        match page.r#type {
            ChannelDisplayPageType::Main => {
                let (channel, is_new) = if let Some(item) = LocalStore::get_info(&page.id) {
                    (item, false)
                } else {
                    (load_channel(&page.id, mainconfig)?, true)
                };

                LocalStore::set_info(page_id.clone(), channel.clone(), is_new);

                let commands = framework
                    .data
                    .global
                    .get::<CommandsConfig>()
                    .unwrap()
                    .channel
                    .clone();

                *self = Self::Main {
                    iteminfo: Box::new(ItemInfo::new(Some(channel.clone()))),
                    channel: Box::new(channel.clone()), // TODO the clone seem rather wasteful here
                    grid: Grid::new(
                        vec![Constraint::Percentage(60), Constraint::Percentage(40)],
                        vec![Constraint::Percentage(100)],
                    )?
                    .border_type(appearance.borders),
                    textlist: Self::new_textlist_with_map(commands.clone()),
                    commands,
                };

                let watch_history = framework.data.global.get_mut::<WatchHistory>().unwrap();
                watch_history.push(channel)?;
            }
            ChannelDisplayPageType::Videos => {
                let channel_videos = SearchProviderWrapper::channel_videos(&page.id)?;
                let continuation = channel_videos.continuation;
                let videos = channel_videos
                    .videos
                    .into_iter()
                    .map(|video| Item::from_common_video(video, mainconfig.image_index))
                    .collect::<Vec<_>>();
                if mainconfig.images.display() {
                    download_all_images(videos.iter().map(|item| item.into()).collect());
                }
                *self = Self::Videos {
                    textlist: TextList::default()
                        .ascii_only(!mainconfig.allow_unicode)
                        .border_type(appearance.borders)
                        .style(Style::default().fg(appearance.colors.text))
                        .items(&videos)?,
                    iteminfo: Box::new(ItemInfo::new(videos.first().cloned())),
                    grid: Grid::new(
                        vec![Constraint::Percentage(60), Constraint::Percentage(40)],
                        vec![Constraint::Percentage(100)],
                    )?
                    .border_type(appearance.borders),
                    videos,
                    continuation: continuation.clone(),
                };
                if let Some(ctoken) = continuation {
                    spawn_videos_prefetch(page_id.clone(), ctoken);
                }
            }
            ChannelDisplayPageType::Playlists => {
                let channel_playlists = SearchProviderWrapper::channel_playlists(&page.id)?;
                let continuation = channel_playlists.continuation;
                let playlists = channel_playlists
                    .playlists
                    .into_iter()
                    .map(Item::from_common_playlist)
                    .collect::<Vec<_>>();
                if mainconfig.images.display() {
                    download_all_images(playlists.iter().map(|item| item.into()).collect());
                }
                *self = Self::Playlists {
                    textlist: TextList::default()
                        .ascii_only(!mainconfig.allow_unicode)
                        .border_type(appearance.borders)
                        .style(Style::default().fg(appearance.colors.text))
                        .items(&playlists)?,
                    iteminfo: Box::new(ItemInfo::new(playlists.first().cloned())),
                    grid: Grid::new(
                        vec![Constraint::Percentage(60), Constraint::Percentage(40)],
                        vec![Constraint::Percentage(100)],
                    )?
                    .border_type(appearance.borders),
                    playlists,
                    continuation: continuation.clone(),
                };
                if let Some(ctoken) = continuation {
                    spawn_playlists_prefetch(page_id.clone(), ctoken);
                }
            }
        }

        let mainconfig = framework.data.global.get::<MainConfig>().unwrap();

        set_envs(
            self.inflate_load(mainconfig, framework.data.global.get::<Status>().unwrap())
                .into_iter(),
            &mut framework.data.state.get_mut::<StateEnvs>().unwrap().0,
        );

        set_envs(
            self.infalte_item_update(mainconfig, framework.data.global.get::<Status>().unwrap())
                .into_iter(),
            &mut framework.data.state.get_mut::<StateEnvs>().unwrap().0,
        );

        Ok(())
    }

    fn mouse_event(
        &mut self,
        framework: &mut tui_additions::framework::FrameworkClean,
        x: u16,
        y: u16,
        _absolute_x: u16,
        _absolute_y: u16,
    ) -> bool {
        let chunk_index = if matches!(self, Self::Main { .. }) {
            1
        } else {
            0
        };
        match self {
            Self::None => return false,
            Self::Main { textlist, grid, .. }
            | Self::Videos { textlist, grid, .. }
            | Self::Playlists { textlist, grid, .. } => {
                let chunk = grid
                    .chunks(
                        if let Some(prev_frame) =
                            framework.data.global.get::<Status>().unwrap().prev_frame
                        {
                            prev_frame
                        } else {
                            return false;
                        },
                    )
                    .unwrap()[0][chunk_index];

                if !chunk.intersects(Rect::new(x, y, 1, 1)) {
                    return false;
                }

                let y = (y - chunk.y) as usize + textlist.scroll;

                if y == textlist.selected
                    || y == textlist.selected + 2
                    || y == textlist.selected + 1
                {
                    self.select_at_cursor(framework);
                    return true;
                }

                if !textlist.items.is_empty() && y > textlist.items.len() + 1 {
                    let _ = textlist.last();
                } else if y <= textlist.selected {
                    textlist.selected = y;
                } else if y >= textlist.selected + 2 {
                    textlist.selected = y - 2;
                }

                self.update();

                framework
                    .data
                    .global
                    .get_mut::<Status>()
                    .unwrap()
                    .render_image = true;
            }
        }
        set_envs(
            self.infalte_item_update(
                framework.data.global.get::<MainConfig>().unwrap(),
                framework.data.global.get::<Status>().unwrap(),
            )
            .into_iter(),
            &mut framework.data.state.get_mut::<StateEnvs>().unwrap().0,
        );

        true
    }
}
