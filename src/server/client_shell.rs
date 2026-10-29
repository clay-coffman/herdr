use ratatui::layout::Rect;

use crate::app;
use crate::protocol::{self, FrameData};

#[cfg(test)]
pub(super) fn snapshot(
    app: &app::App,
    boot_id: &str,
    revision: u64,
    config_diagnostic: Option<&str>,
    location: Option<&crate::server::clients::ClientShellLocation>,
) -> protocol::ClientShellSnapshot {
    snapshot_with_completions(app, boot_id, revision, config_diagnostic, location).0
}

pub(super) fn snapshot_with_completions(
    app: &app::App,
    boot_id: &str,
    revision: u64,
    config_diagnostic: Option<&str>,
    location: Option<&crate::server::clients::ClientShellLocation>,
) -> (
    protocol::ClientShellSnapshot,
    protocol::endpoint::EndpointAgentCompletions,
) {
    let snapshot = app.session_metadata_snapshot();
    let completions = protocol::endpoint::EndpointAgentCompletions {
        boot_id: boot_id.to_owned(),
        revision,
        completions: snapshot
            .agents
            .iter()
            .filter_map(|agent| agent.completion_seq.map(|seq| (agent.pane_id.clone(), seq)))
            .collect(),
    };
    let focused_workspace_id = location
        .and_then(|location| location.focused_workspace_id.clone())
        .or_else(|| snapshot.focused_workspace_id.clone());
    let focused_tab_id = location
        .and_then(|location| location.focused_tab_id().map(str::to_owned))
        .or_else(|| snapshot.focused_tab_id.clone());
    let focused_pane_id = focused_tab_id
        .as_deref()
        .and_then(|tab_id| app.parse_tab_id(tab_id))
        .and_then(|(workspace_index, tab_index)| {
            let pane_id = app
                .state
                .workspaces
                .get(workspace_index)?
                .tabs
                .get(tab_index)?
                .layout
                .focused();
            app.public_pane_id(workspace_index, pane_id)
        })
        .or_else(|| snapshot.focused_pane_id.clone());
    let workspaces: Vec<protocol::ClientShellWorkspace> = snapshot
        .workspaces
        .into_iter()
        .zip(&app.state.workspaces)
        .enumerate()
        .map(|(workspace_index, (workspace, state))| {
            let mut tokens = workspace.tokens.into_iter().collect::<Vec<_>>();
            tokens.sort_by(|left, right| left.0.cmp(&right.0));
            let workspace_id = workspace.workspace_id;
            let active_tab_id = location
                .and_then(|location| location.active_tab_ids.get(&workspace_id))
                .cloned()
                .unwrap_or(workspace.active_tab_id);
            let active_tab_index =
                app.parse_tab_id(&active_tab_id)
                    .and_then(|(tab_workspace_index, tab_index)| {
                        (tab_workspace_index == workspace_index).then_some(tab_index)
                    });
            protocol::ClientShellWorkspace {
                focused: focused_workspace_id.as_deref() == Some(workspace_id.as_str()),
                workspace_id,
                active_tab_id,
                new_workspace_cwd: app
                    .resolved_new_workspace_cwd_from_tab(workspace_index, active_tab_index)
                    .display()
                    .to_string(),
                number: workspace.number,
                label: workspace.label,
                custom_label: state.custom_name.is_some(),
                branch: state.branch(),
                git_ahead_behind: state.git_ahead_behind(),
                tokens,
                worktree: workspace
                    .worktree
                    .map(|worktree| protocol::ClientShellWorktree {
                        key: worktree.repo_key,
                        label: worktree.repo_name,
                        is_linked_worktree: worktree.is_linked_worktree,
                    }),
                agent_status: workspace.agent_status,
            }
        })
        .collect();
    let tabs = snapshot
        .tabs
        .into_iter()
        .zip(
            app.state
                .workspaces
                .iter()
                .flat_map(|workspace| workspace.tabs.iter()),
        )
        .map(|(tab, state)| {
            let tab_id = tab.tab_id;
            protocol::ClientShellTab {
                focused: focused_tab_id.as_deref() == Some(tab_id.as_str()),
                tab_id,
                workspace_id: tab.workspace_id,
                number: tab.number,
                label: tab.label,
                custom_label: !state.is_auto_named(),
                zoomed: state.zoomed,
                agent_status: tab.agent_status,
            }
        })
        .collect();
    let panes = snapshot
        .panes
        .into_iter()
        .map(|pane| {
            let pane_id = pane.pane_id;
            let focused = focused_pane_id.as_deref() == Some(pane_id.as_str());
            let right_click_passthrough = app
                .parse_pane_id(&pane_id)
                .and_then(|(workspace_index, pane_id)| {
                    app.state
                        .workspaces
                        .get(workspace_index)?
                        .pane_state(pane_id)
                })
                .is_some_and(|pane| pane.right_click_passthrough);
            protocol::ClientShellPane {
                pane_id,
                workspace_id: pane.workspace_id,
                tab_id: pane.tab_id,
                label: pane.label,
                cwd: pane.cwd,
                foreground_cwd: pane.foreground_cwd,
                focused,
                right_click_passthrough,
            }
        })
        .collect();
    let agents = snapshot
        .agents
        .into_iter()
        .map(|agent| {
            let pane_id = agent.pane_id;
            let focused = focused_pane_id.as_deref() == Some(pane_id.as_str());
            let mut state_labels = agent.state_labels.into_iter().collect::<Vec<_>>();
            state_labels.sort_by(|left, right| left.0.cmp(&right.0));
            let mut tokens = agent.tokens.into_iter().collect::<Vec<_>>();
            tokens.sort_by(|left, right| left.0.cmp(&right.0));
            protocol::ClientShellAgent {
                pane_id,
                workspace_id: agent.workspace_id,
                tab_id: agent.tab_id,
                name: agent.name,
                display_agent: agent.display_agent,
                agent: agent.agent,
                title: agent.title,
                terminal_title: agent.terminal_title,
                terminal_title_stripped: agent.terminal_title_stripped,
                agent_status: agent.agent_status,
                state_change_seq: agent.state_change_seq,
                state_labels,
                tokens,
                focused,
            }
        })
        .collect();

    let agent_view_label = app
        .state
        .agent_view_override
        .as_ref()
        .map(|view| view.label.clone().unwrap_or_else(|| "filtered".to_owned()));
    let agent_order = crate::ui::agent_panel_entries_from(&app.state, &app.terminal_runtimes)
        .into_iter()
        .filter_map(|entry| app.public_pane_id(entry.ws_idx, entry.pane_id))
        .collect();

    let zoomed = focused_tab_id
        .as_deref()
        .and_then(|tab_id| app.parse_tab_id(tab_id))
        .and_then(|(workspace_index, tab_index)| {
            app.state
                .workspaces
                .get(workspace_index)?
                .tabs
                .get(tab_index)
        })
        .is_some_and(|tab| tab.zoomed);
    let workspace_status = workspaces
        .iter()
        .find(|workspace| workspace.focused)
        .map(|workspace| compact_workspace_label(&workspace.label))
        .filter(|text| !text.is_empty());
    let tab_bar_right = app
        .state
        .tab_bar_right
        .iter()
        .filter_map(|segment| match segment {
            crate::app::state::TabBarStatusSegment::Zoom if zoomed => {
                Some(protocol::ClientShellTabStatusSegment {
                    text: "ZOOM".to_owned(),
                    accent: true,
                })
            }
            crate::app::state::TabBarStatusSegment::Workspace => {
                workspace_status
                    .as_ref()
                    .map(|text| protocol::ClientShellTabStatusSegment {
                        text: text.clone(),
                        accent: true,
                    })
            }
            crate::app::state::TabBarStatusSegment::Text(Some(text)) if !text.is_empty() => {
                Some(protocol::ClientShellTabStatusSegment {
                    text: text.clone(),
                    accent: false,
                })
            }
            crate::app::state::TabBarStatusSegment::Zoom
            | crate::app::state::TabBarStatusSegment::Text(_) => None,
        })
        .collect();

    let product_announcement = app.state.product_announcement.as_ref().map(|announcement| {
        protocol::ClientShellProductAnnouncement {
            version: announcement.version.clone(),
            id: announcement.id.clone(),
            title: announcement.title.clone(),
            body: announcement.body.clone(),
            preview: announcement.preview,
        }
    });
    let release_notes =
        app.state
            .latest_release_notes
            .as_ref()
            .map(|notes| protocol::ClientShellReleaseNotes {
                version: notes.version.clone(),
                body: notes.body.clone(),
                preview: notes.preview,
            });

    let shell = protocol::ClientShellSnapshot {
        boot_id: boot_id.to_owned(),
        revision,
        config_diagnostic: config_diagnostic.map(str::to_owned),
        product_announcement,
        update_available: app.state.update_available.clone(),
        update_install_command: app.state.update_install_command.clone(),
        server_keybindings_toml: app.client_shell_keybindings_profile().map(str::to_owned),
        latest_release_notes_available: app.state.latest_release_notes_available,
        integration_updates_available: app.state.integration_updates_available(),
        worktree_directory: app.state.worktree_directory.to_string_lossy().into_owned(),
        release_notes,
        focused_workspace_id,
        focused_tab_id,
        focused_pane_id,
        tab_bar_right,
        tab_bar_right_separator: app.state.tab_bar_right_separator.clone(),
        agent_view_label,
        agent_order,
        workspaces,
        tabs,
        panes,
        agents,
        commands: app.client_shell_command_manifest(),
    };
    (shell, completions)
}

pub(super) struct RenderedPaneSurface {
    pub(super) frame: FrameData,
    pub(super) panes: Vec<protocol::PaneSurfacePane>,
    pub(super) splits: Vec<protocol::PaneSurfaceSplit>,
    pub(super) popup: Option<Box<protocol::ClientShellPopupSurface>>,
    pub(super) graphics: protocol::SurfaceGraphicsScene,
    pub(super) graphics_delivery: crate::kitty_graphics::surface::DeliveryCache,
    pub(super) graphics_sources: crate::kitty_graphics::surface::SourceFiles,
}

#[derive(Debug)]
pub(super) enum SurfaceRenderDeferred {
    Synchronized,
    Changed,
}

pub(super) fn render_pane_surface(
    app: &mut app::App,
    target: Option<crate::ui::TabSurfaceTarget>,
    area: Rect,
    resize_panes: bool,
    show_popup: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
    graphics_delivery: &crate::kitty_graphics::surface::DeliveryCache,
    client_id: u64,
) -> Result<RenderedPaneSurface, SurfaceRenderDeferred> {
    let layout = crate::ui::compute_tab_surface_for(
        &app.state,
        &app.terminal_runtimes,
        target,
        area,
        resize_panes,
        cell_size,
    );
    let mut content_revisions_before = std::collections::HashMap::new();
    if let Some(target) = target {
        for pane in &layout.pane_infos {
            if let Some(runtime) = app.state.runtime_for_pane_in_workspace(
                &app.terminal_runtimes,
                target.workspace_index,
                pane.id,
            ) {
                let (synchronized, epoch) = runtime.synchronized_output_state();
                if synchronized {
                    return Err(SurfaceRenderDeferred::Synchronized);
                }
                let revision = runtime.content_seq();
                content_revisions_before.insert(pane.id, (epoch, revision));
            }
        }
    }
    let popup_revision_before = if show_popup {
        app.state
            .popup_pane
            .as_ref()
            .and_then(|popup| app.terminal_runtimes.get(&popup.terminal_id))
            .map(|runtime| {
                let (synchronized, epoch) = runtime.synchronized_output_state();
                if synchronized {
                    return Err(SurfaceRenderDeferred::Synchronized);
                }
                Ok(epoch)
            })
            .transpose()?
    } else {
        None
    };
    let (buffer, cursor, hyperlinks, layout) =
        crate::server::render_stream::render_tab_surface_virtual(
            &app.state,
            &app.terminal_runtimes,
            layout,
            area,
        );
    let panes = target
        .map(|target| {
            let workspace_index = target.workspace_index;
            layout
                .pane_infos
                .iter()
                .filter_map(|pane| {
                    app.public_pane_id(workspace_index, pane.id).map(|pane_id| {
                        let runtime = app.state.runtime_for_pane_in_workspace(
                            &app.terminal_runtimes,
                            workspace_index,
                            pane.id,
                        );
                        let mouse_reporting =
                            runtime.is_some_and(|runtime| runtime.mouse_reporting_enabled());
                        let sgr_pixel_mouse =
                            runtime.is_some_and(|runtime| runtime.sgr_pixel_mouse_enabled());
                        let (pixel_width, pixel_height) = if cell_size.is_known() {
                            (
                                u32::from(pane.inner_rect.width) * cell_size.width_px,
                                u32::from(pane.inner_rect.height) * cell_size.height_px,
                            )
                        } else {
                            (0, 0)
                        };
                        let content_revision = runtime.map_or(0, |runtime| {
                            let after = runtime.content_seq();
                            if content_revisions_before
                                .get(&pane.id)
                                .is_some_and(|&(_, before)| before == after)
                                && after.is_multiple_of(2)
                            {
                                after
                            } else {
                                after | 1
                            }
                        });
                        protocol::PaneSurfacePane {
                            pane_id,
                            content_revision,
                            rect: pane.rect.into(),
                            inner_rect: pane.inner_rect.into(),
                            scrollbar_rect: pane.scrollbar_rect.map(Into::into),
                            scroll: runtime.and_then(|runtime| runtime.scroll_metrics()).map(
                                |metrics| protocol::PaneSurfaceScrollMetrics {
                                    offset_from_bottom: metrics.offset_from_bottom as u64,
                                    max_offset_from_bottom: metrics.max_offset_from_bottom as u64,
                                    viewport_rows: metrics.viewport_rows as u64,
                                },
                            ),
                            focused: pane.is_focused,
                            mouse_reporting,
                            sgr_pixel_mouse,
                            alternate_screen_active: runtime
                                .is_some_and(|runtime| runtime.alternate_screen_active()),
                            pixel_width,
                            pixel_height,
                        }
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    let pane_frames = layout
        .pane_infos
        .iter()
        .map(|pane| pane.rect)
        .collect::<Vec<_>>();
    let splits = layout
        .split_borders
        .iter()
        .filter_map(|split| {
            let hit_rect = split_hit_rect(
                split,
                app.state.pane_borders.draws_borders(),
                app.state.pane_gaps,
                &pane_frames,
            )?;
            let direction = match split.direction {
                ratatui::layout::Direction::Horizontal => {
                    protocol::PaneSurfaceSplitDirection::Horizontal
                }
                ratatui::layout::Direction::Vertical => {
                    protocol::PaneSurfaceSplitDirection::Vertical
                }
            };
            Some(protocol::PaneSurfaceSplit {
                direction,
                pos: split.pos,
                area: split.area.into(),
                hit_rect: hit_rect.into(),
                path: split.path.clone(),
            })
        })
        .collect();
    let popup = show_popup
        .then(|| render_popup_surface(app, area, resize_panes, cell_size))
        .flatten();
    let (graphics, next_graphics_delivery, graphics_sources) =
        crate::server::client_shell_graphics::collect(
            app,
            &layout.pane_infos,
            &layout.split_borders,
            popup.as_deref(),
            target,
            cell_size,
            graphics_delivery,
            client_id,
        );
    if let Some(target) = target {
        for (&pane_id, &(epoch, _)) in &content_revisions_before {
            if let Some(runtime) = app.state.runtime_for_pane_in_workspace(
                &app.terminal_runtimes,
                target.workspace_index,
                pane_id,
            ) {
                let (synchronized, after_epoch) = runtime.synchronized_output_state();
                if synchronized {
                    return Err(SurfaceRenderDeferred::Synchronized);
                }
                if after_epoch != epoch {
                    return Err(SurfaceRenderDeferred::Changed);
                }
            }
        }
    }
    if let Some(before) = popup_revision_before {
        if let Some(runtime) = app
            .state
            .popup_pane
            .as_ref()
            .and_then(|popup| app.terminal_runtimes.get(&popup.terminal_id))
        {
            let (synchronized, after_epoch) = runtime.synchronized_output_state();
            if synchronized {
                return Err(SurfaceRenderDeferred::Synchronized);
            }
            if after_epoch != before {
                return Err(SurfaceRenderDeferred::Changed);
            }
        }
    }
    Ok(RenderedPaneSurface {
        frame: FrameData::from_ratatui_buffer_with_hyperlinks(&buffer, cursor, &hyperlinks),
        panes,
        splits,
        popup,
        graphics,
        graphics_delivery: next_graphics_delivery,
        graphics_sources,
    })
}

fn render_popup_surface(
    app: &app::App,
    area: Rect,
    resize_runtime: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
) -> Option<Box<protocol::ClientShellPopupSurface>> {
    let popup = app.state.popup_pane.as_ref()?;
    let geometry = if resize_runtime {
        resize_popup_runtime(app, area, cell_size)?
    } else {
        crate::popup_size::resolve_popup_geometry(popup.width, popup.height, area)?
    };
    let runtime = app.terminal_runtimes.get(&popup.terminal_id)?;
    let content_area = Rect::new(0, 0, geometry.inner.width, geometry.inner.height);
    let (buffer, cursor) =
        crate::server::render_stream::render_terminal_virtual(runtime, content_area);
    let hyperlinks = runtime.visible_hyperlinks(content_area);
    let title = app
        .state
        .terminals
        .get(&popup.terminal_id)
        .and_then(|terminal| terminal.manual_label.clone())
        .unwrap_or_else(|| "popup".to_owned());
    let (pixel_width, pixel_height) = if cell_size.is_known() {
        (
            u32::from(content_area.width) * cell_size.width_px,
            u32::from(content_area.height) * cell_size.height_px,
        )
    } else {
        (0, 0)
    };
    Some(Box::new(protocol::ClientShellPopupSurface {
        terminal_id: popup.terminal_id.to_string(),
        title,
        width: popup.width.map(client_popup_size),
        height: popup.height.map(client_popup_size),
        frame: FrameData::from_ratatui_buffer_with_hyperlinks(&buffer, cursor, &hyperlinks),
        mouse_reporting: runtime.mouse_reporting_enabled(),
        sgr_pixel_mouse: runtime.sgr_pixel_mouse_enabled(),
        pixel_width,
        pixel_height,
    }))
}

pub(super) fn resize_popup_runtime(
    app: &app::App,
    area: Rect,
    cell_size: crate::kitty_graphics::HostCellSize,
) -> Option<crate::popup_size::PopupResolvedGeometry> {
    let popup = app.state.popup_pane.as_ref()?;
    let geometry = crate::popup_size::resolve_popup_geometry(popup.width, popup.height, area)?;
    let runtime = app.terminal_runtimes.get(&popup.terminal_id)?;
    if !app
        .state
        .direct_attach_resize_locks
        .contains(&popup.terminal_id)
    {
        runtime.resize(
            geometry.inner.height,
            geometry.inner.width,
            cell_size.width_px,
            cell_size.height_px,
        );
    }
    Some(geometry)
}

fn client_popup_size(size: crate::popup_size::PopupSize) -> protocol::ClientShellPopupSize {
    match size {
        crate::popup_size::PopupSize::Cells(cells) => protocol::ClientShellPopupSize::Cells(cells),
        crate::popup_size::PopupSize::Percent(percent) => {
            protocol::ClientShellPopupSize::Percent(percent)
        }
    }
}

fn split_hit_rect(
    split: &crate::layout::SplitBorder,
    pane_borders: bool,
    pane_gaps: bool,
    pane_frames: &[Rect],
) -> Option<Rect> {
    let hit = match (split.direction, pane_borders, pane_gaps) {
        (ratatui::layout::Direction::Horizontal, true, false) => {
            Rect::new(split.pos, split.area.y, 1, split.area.height)
        }
        (ratatui::layout::Direction::Horizontal, true, true) => {
            let start = split.pos.saturating_sub(1);
            Rect::new(
                start,
                split.area.y,
                split.pos.saturating_sub(start).saturating_add(1),
                split.area.height,
            )
        }
        (ratatui::layout::Direction::Horizontal, false, true) => Rect::new(
            split.pos.checked_sub(1)?,
            split.area.y,
            1,
            split.area.height,
        ),
        (ratatui::layout::Direction::Vertical, true, false) => {
            Rect::new(split.area.x, split.pos, split.area.width, 1)
        }
        (ratatui::layout::Direction::Vertical, true, true) => {
            let start = split.pos.saturating_sub(1);
            Rect::new(
                split.area.x,
                start,
                split.area.width,
                split.pos.saturating_sub(start).saturating_add(1),
            )
        }
        (ratatui::layout::Direction::Vertical, false, true) => {
            Rect::new(split.area.x, split.pos.checked_sub(1)?, split.area.width, 1)
        }
        (_, false, false) => return None,
    };
    if !pane_borders
        && pane_frames.iter().any(|pane| {
            hit.x < pane.right()
                && hit.right() > pane.x
                && hit.y < pane.bottom()
                && hit.bottom() > pane.y
        })
    {
        return None;
    }
    Some(hit)
}

/// What the client keeps for its tab strip. The client shows the right-hand
/// status only when `status width + 1 + MIN_TAB_STRIP_WIDTH (17)` fits its tab
/// bar and otherwise hides all of it (`client/shell/tabs.rs`). Two more columns
/// cover a client built from a slightly different Herdr.
const CLIENT_TAB_STRIP_RESERVE: u16 = 17 + 1 + 2;
const MIN_ACCENT_STATUS_COLUMNS: usize = 4;
const MAX_WORKSPACE_STATUS_COLUMNS: usize = 60;

/// Fit the tab bar status to one client's width, so the client never hides it
/// whole. Plain segments (commands, hostname, clock, text) go first, from the
/// end; the accented ones are this client's own state (its workspace, ZOOM),
/// and the widest of them is shortened with an ellipsis as a last resort. The
/// server cannot see whether the client's sidebar is open; when it is, the
/// sidebar names the workspace anyway.
pub(super) fn fit_tab_bar_right(snapshot: &mut protocol::ClientShellSnapshot, cols: u16) {
    use crate::ui::text::display_width;
    let budget = usize::from(cols.saturating_sub(CLIENT_TAB_STRIP_RESERVE));
    let separator = display_width(&snapshot.tab_bar_right_separator);
    let total = |segments: &[&protocol::ClientShellTabStatusSegment]| -> usize {
        segments
            .iter()
            .map(|segment| display_width(&segment.text))
            .sum::<usize>()
            + separator * segments.len().saturating_sub(1)
    };
    let segments = &mut snapshot.tab_bar_right;
    if total(&segments.iter().collect::<Vec<_>>()) <= budget {
        return;
    }
    // This client's own segments first, then each plain one, in order, that
    // still fits: a wide command line gives way to the hostname and clock.
    let mut keep: Vec<bool> = segments.iter().map(|segment| segment.accent).collect();
    for index in 0..segments.len() {
        if keep[index] {
            continue;
        }
        keep[index] = true;
        let chosen: Vec<_> = segments
            .iter()
            .zip(&keep)
            .filter_map(|(segment, keep)| keep.then_some(segment))
            .collect();
        if total(&chosen) > budget {
            keep[index] = false;
        }
    }
    let mut keep = keep.into_iter();
    segments.retain(|_| keep.next().unwrap_or(false));
    loop {
        let width_now = total(&segments.iter().collect::<Vec<_>>());
        if width_now <= budget {
            return;
        }
        let Some(widest) =
            (0..segments.len()).max_by_key(|&index| display_width(&segments[index].text))
        else {
            return;
        };
        let width = display_width(&segments[widest].text);
        let room = budget.saturating_sub(width_now - width);
        if room >= MIN_ACCENT_STATUS_COLUMNS.min(width) {
            segments[widest].text = shorten(&segments[widest].text, room);
            return;
        }
        if segments.len() > 1 {
            let keep = segments.remove(widest);
            segments.clear();
            segments.push(keep);
            continue;
        }
        segments.clear();
    }
}

/// `truncate_end`, without leaving a space or separator before the ellipsis.
fn shorten(text: &str, max_width: usize) -> String {
    let short = crate::ui::text::truncate_end(text, max_width);
    match short.strip_suffix('…') {
        Some(prefix) if prefix.len() < text.len() => {
            let prefix = prefix.trim_end_matches([' ', '·', ':', '-', '—', '|', ',']);
            if prefix.is_empty() {
                short
            } else {
                format!("{prefix}…")
            }
        }
        _ => short,
    }
}

/// A workspace label as the tab bar shows it: an issue key first when the
/// label has one, then the name it belongs to. Tower labels workspaces
/// `[3] Restricted charts and emergency access · LUM-2291 Restricted-access…`
/// (catalog number, workstream, issue, title) or `[6] LUM-700 Internal…`; those
/// become `LUM-2291 · Restricted charts and emergency access` and
/// `LUM-700 · Internal…`. Any other label is shown as it is.
fn compact_workspace_label(label: &str) -> String {
    let body = strip_catalog_number(label.trim());
    let separators: &[char] = &['·', ':', '-', '—', '|'];
    let text = match find_issue_key(body) {
        Some((start, end)) => {
            let key = &body[start..end];
            let before = body[..start]
                .trim_end()
                .trim_end_matches(separators)
                .trim_end();
            let after = body[end..]
                .trim_start()
                .trim_start_matches(separators)
                .trim_start();
            let name = if before.is_empty() { after } else { before };
            if name.is_empty() {
                key.to_owned()
            } else {
                format!("{key} · {name}")
            }
        }
        None => body.to_owned(),
    };
    shorten(&text, MAX_WORKSPACE_STATUS_COLUMNS)
}

fn strip_catalog_number(label: &str) -> &str {
    let Some(rest) = label.strip_prefix('[') else {
        return label;
    };
    let Some((number, rest)) = rest.split_once(']') else {
        return label;
    };
    if number.is_empty() || !number.chars().all(|ch| ch.is_ascii_digit()) {
        return label;
    }
    let rest = rest.trim_start();
    if rest.is_empty() {
        label
    } else {
        rest
    }
}

/// Byte range of the first word shaped like an issue key (`LUM-2291`,
/// `ENG-7`): 2–10 upper-case letters or digits starting with a letter, a
/// hyphen, and 1–7 digits.
fn find_issue_key(text: &str) -> Option<(usize, usize)> {
    let mut start = None;
    for (index, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), ' ')))
    {
        let word = ch.is_ascii_alphanumeric() || ch == '-';
        match (start, word) {
            (None, true) => start = Some(index),
            (Some(begin), false) => {
                if is_issue_key(&text[begin..index]) {
                    return Some((begin, index));
                }
                start = None;
            }
            _ => {}
        }
    }
    None
}

fn is_issue_key(word: &str) -> bool {
    let Some((team, number)) = word.split_once('-') else {
        return false;
    };
    (2..=10).contains(&team.len())
        && team.starts_with(|ch: char| ch.is_ascii_uppercase())
        && team
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit())
        && (1..=7).contains(&number.len())
        && number.chars().all(|ch| ch.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status(text: &str, accent: bool) -> protocol::ClientShellTabStatusSegment {
        protocol::ClientShellTabStatusSegment {
            text: text.to_owned(),
            accent,
        }
    }

    #[test]
    fn workspace_labels_lead_with_the_issue_key() {
        let cases = [
            (
                "[3] Restricted charts and emergency access · LUM-2291 Restricted-access experience redesign and access-enforcement API audit",
                "LUM-2291 · Restricted charts and emergency access",
            ),
            (
                "[6] LUM-700 Internal operator authentication, accounts and permissions",
                "LUM-700 · Internal operator authentication, accounts and pe…",
            ),
            (
                "Real text delivery in ehr-dev · LUM-2332 Prove real text delivery in ehr-dev",
                "LUM-2332 · Real text delivery in ehr-dev",
            ),
            ("[1] EHR · Coordinator", "EHR · Coordinator"),
            ("Tower · Agent coordination", "Tower · Agent coordination"),
            ("hangar", "hangar"),
            ("[12]", "[12]"),
            ("LUM-9", "LUM-9"),
            ("feat/lum-2291 Restricted-access ehr-dev", "feat/lum-2291 Restricted-access ehr-dev"),
        ];
        for (label, expected) in cases {
            assert_eq!(compact_workspace_label(label), expected, "{label}");
        }
        let long = compact_workspace_label(&format!("[2] {} · LUM-606 Order", "x".repeat(90)));
        assert!(long.starts_with("LUM-606 · xxx"));
        assert!(long.ends_with('…'));
        assert_eq!(
            crate::ui::text::display_width(&long),
            MAX_WORKSPACE_STATUS_COLUMNS
        );
    }

    #[test]
    fn tab_bar_fits_every_width_and_keeps_the_workspace_longest() {
        let full = vec![
            status("LUM-2291 · Restricted charts and emergency access", true),
            status(
                "◔1 ✓2 ✓3 ✓4 ✓5 ◔6 ✓7 ⚑8 ✓9 ✓10 ⚑11 ⚑12 ✓13 ◔14 │ ⚑ 3 need you",
                false,
            ),
            status("ip-10-42-7-137", false),
            status("20:48", false),
        ];
        let mut previous_workspace_width = usize::MAX;
        for cols in (0..=240u16).rev() {
            let mut snapshot = snapshot_for_tab_bar(full.clone());
            fit_tab_bar_right(&mut snapshot, cols);
            let width = client_status_width(&snapshot);
            // The client's rule: status + 1 + its 17-column tab strip must fit.
            assert!(
                width == 0 || width + 1 + 17 <= usize::from(cols),
                "cols {cols}: status {width} would be hidden"
            );
            match snapshot.tab_bar_right.first() {
                Some(first) => {
                    assert!(first.accent, "cols {cols}: the workspace goes last");
                    let workspace = crate::ui::text::display_width(&first.text);
                    assert!(workspace <= previous_workspace_width);
                    previous_workspace_width = workspace;
                }
                None => assert!(cols < 24, "cols {cols}: nothing shown"),
            }
        }
        let mut wide = snapshot_for_tab_bar(full.clone());
        fit_tab_bar_right(&mut wide, 240);
        assert_eq!(wide.tab_bar_right, full);
        let mut narrow = snapshot_for_tab_bar(full.clone());
        fit_tab_bar_right(&mut narrow, 60);
        assert_eq!(
            narrow.tab_bar_right,
            vec![status("LUM-2291 · Restricted charts and emerge…", true)]
        );
        let mut medium = snapshot_for_tab_bar(full.clone());
        fit_tab_bar_right(&mut medium, 100);
        assert_eq!(
            medium.tab_bar_right,
            vec![full[0].clone(), full[2].clone(), full[3].clone()],
            "the rail gives way to the hostname and clock"
        );
    }

    /// The client's `tab_bar_status_width` (client/shell/tabs.rs).
    fn client_status_width(snapshot: &protocol::ClientShellSnapshot) -> usize {
        let text: usize = snapshot
            .tab_bar_right
            .iter()
            .map(|segment| crate::ui::text::display_width(&segment.text))
            .sum();
        text + crate::ui::text::display_width(&snapshot.tab_bar_right_separator)
            * snapshot.tab_bar_right.len().saturating_sub(1)
    }

    #[test]
    fn tab_bar_fit_drops_zoom_before_the_workspace_becomes_unreadable() {
        let mut snapshot = snapshot_for_tab_bar(vec![
            status("ZOOM", true),
            status("LUM-2291 · Restricted charts", true),
            status("20:48", false),
        ]);
        fit_tab_bar_right(&mut snapshot, 30);
        assert_eq!(snapshot.tab_bar_right, vec![status("LUM-2291…", true)]);
    }

    fn snapshot_for_tab_bar(
        segments: Vec<protocol::ClientShellTabStatusSegment>,
    ) -> protocol::ClientShellSnapshot {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let app = crate::app::App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        let mut snapshot = snapshot(&app, "boot", 1, None, None);
        snapshot.tab_bar_right = segments;
        snapshot.tab_bar_right_separator = " │ ".into();
        snapshot
    }

    #[test]
    fn workspace_status_follows_each_clients_own_workspace() {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut config = crate::config::Config::default();
        config.ui.tab_bar_right = vec![
            crate::config::TabBarRightEntryConfig::Workspace,
            crate::config::TabBarRightEntryConfig::Text {
                text: "shared".into(),
            },
        ];
        let mut app = crate::app::App::new(
            &config,
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        let mut lead = crate::workspace::Workspace::test_new("lead");
        lead.custom_name =
            Some("[3] Restricted charts and emergency access · LUM-2291 Restricted-access".into());
        let mut tool = crate::workspace::Workspace::test_new("tool");
        tool.custom_name = Some("hangar".into());
        app.state.workspaces = vec![lead, tool];
        app.state.active = Some(0);
        app.state.ensure_test_terminals();

        let server_view = snapshot(&app, "boot", 1, None, None);
        assert_eq!(
            server_view.tab_bar_right,
            vec![
                status("LUM-2291 · Restricted charts and emergency access", true),
                status("shared", false),
            ]
        );
        let mut location = crate::server::clients::ClientShellLocation::from_snapshot(&server_view);
        location.focused_workspace_id = Some(server_view.workspaces[1].workspace_id.clone());
        let other_window = snapshot(&app, "boot", 1, None, Some(&location));
        assert_eq!(
            other_window.tab_bar_right,
            vec![status("hangar", true), status("shared", false)]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn snapshot_metadata_skips_scroll_reads_and_preserves_public_session_data() {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = crate::app::App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        let mut workspace = crate::workspace::Workspace::test_new("active");
        workspace.test_add_tab(None);
        app.state.workspaces = vec![workspace, crate::workspace::Workspace::test_new("hidden")];
        app.state.active = Some(0);
        app.state.ensure_test_terminals();
        let history = (0..40)
            .map(|line| format!("line-{line}\r\n"))
            .collect::<String>();
        for (index, terminal) in app.state.terminals.values_mut().enumerate() {
            if index < 2 {
                terminal.set_detected_state(
                    Some(crate::detect::Agent::Pi),
                    crate::detect::AgentState::Working,
                );
                terminal.agent_name = Some(format!("agent-{index}"));
                terminal.last_agent_state_change_seq = Some(5);
                terminal.last_agent_completion_seq = Some(7);
            }
            let runtime = crate::terminal::TerminalRuntime::test_with_scrollback_bytes(
                20,
                5,
                1000,
                history.as_bytes(),
            );
            runtime.scroll_up(3);
            app.terminal_runtimes.insert(terminal.id.clone(), runtime);
        }

        let metadata = app.session_metadata_snapshot();
        assert_eq!(metadata.panes.len(), 3);
        assert_eq!(metadata.agents.len(), 2);
        assert!(metadata.panes.iter().all(|pane| pane.scroll.is_none()));
        assert!(app
            .terminal_runtimes
            .values()
            .all(|runtime| runtime.test_scroll_metrics_reads() == 0));

        let mut public = app.session_snapshot();
        for pane in &mut public.panes {
            let scroll = pane
                .scroll
                .take()
                .expect("public session must expose scroll");
            assert_eq!(scroll.offset_from_bottom, 3);
            assert!(scroll.max_offset_from_bottom >= 3);
            assert_eq!(scroll.viewport_rows, 5);
        }
        assert_eq!(metadata, public);
        assert!(app
            .terminal_runtimes
            .values()
            .all(|runtime| runtime.test_scroll_metrics_reads() == 1));

        let (shell, completions) = snapshot_with_completions(&app, "boot", 9, None, None);
        assert_eq!(shell.panes.len(), 3);
        assert_eq!(shell.agents.len(), 2);
        assert_eq!(shell.focused_pane_id, metadata.focused_pane_id);
        for (pane, public_pane) in shell.panes.iter().zip(&metadata.panes) {
            assert_eq!(pane.pane_id, public_pane.pane_id);
            assert_eq!(pane.workspace_id, public_pane.workspace_id);
            assert_eq!(pane.tab_id, public_pane.tab_id);
            assert_eq!(pane.cwd, public_pane.cwd);
        }
        for (agent, public_agent) in shell.agents.iter().zip(&metadata.agents) {
            assert_eq!(agent.pane_id, public_agent.pane_id);
            assert_eq!(agent.name, public_agent.name);
            assert_eq!(agent.agent_status, public_agent.agent_status);
            assert_eq!(agent.state_change_seq, 5);
        }
        assert_eq!(completions.revision, 9);
        assert_eq!(completions.completions.len(), 2);
        assert!(completions.completions.iter().all(|(_, seq)| *seq == 7));
        assert!(app
            .terminal_runtimes
            .values()
            .all(|runtime| runtime.test_scroll_metrics_reads() == 1));
    }

    #[test]
    fn snapshot_projects_cached_release_and_update_facts() {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = crate::app::App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.integration_recommendations.clear();
        app.state.update_available = Some("0.8.3".into());
        app.state.update_install_command = "herdr update".into();
        app.state.latest_release_notes_available = true;
        app.state.latest_release_notes = Some(crate::release_notes::ReleaseNotes {
            version: "0.8.3".into(),
            body: "### Changed\n- Client shell".into(),
            preview: true,
        });

        let snapshot = snapshot(&app, "boot", 7, None, None);

        assert_eq!(snapshot.update_available.as_deref(), Some("0.8.3"));
        assert_eq!(snapshot.update_install_command, "herdr update");
        assert!(snapshot.latest_release_notes_available);
        assert!(!snapshot.integration_updates_available);
        assert_eq!(
            snapshot.release_notes.as_ref().map(|notes| (
                notes.version.as_str(),
                notes.body.as_str(),
                notes.preview
            )),
            Some(("0.8.3", "### Changed\n- Client shell", true))
        );
    }

    #[test]
    fn snapshot_badges_only_outdated_integrations() {
        let (_api_tx, api_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = crate::app::App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            api_rx,
            crate::api::EventHub::default(),
        );
        app.state.integration_recommendations =
            vec![crate::integration::IntegrationRecommendation {
                target: crate::api::schema::IntegrationTarget::Claude,
                label: "claude",
                command: "claude",
                available: true,
                path: std::path::PathBuf::from("claude-hook"),
                state: crate::integration::IntegrationStatusKind::NotInstalled,
            }];

        assert!(!snapshot(&app, "boot", 1, None, None).integration_updates_available);

        app.state.integration_recommendations[0].state =
            crate::integration::IntegrationStatusKind::Outdated;
        assert!(snapshot(&app, "boot", 2, None, None).integration_updates_available);
    }

    #[test]
    fn split_hits_follow_released_border_and_gap_geometry() {
        let horizontal = crate::layout::SplitBorder {
            pos: 20,
            direction: ratatui::layout::Direction::Horizontal,
            ratio: 0.5,
            area: Rect::new(2, 3, 40, 12),
            path: vec![false],
        };
        assert_eq!(
            split_hit_rect(&horizontal, true, false, &[]),
            Some(Rect::new(20, 3, 1, 12))
        );
        assert_eq!(
            split_hit_rect(&horizontal, true, true, &[]),
            Some(Rect::new(19, 3, 2, 12))
        );
        assert_eq!(
            split_hit_rect(&horizontal, false, true, &[]),
            Some(Rect::new(19, 3, 1, 12))
        );
        assert_eq!(split_hit_rect(&horizontal, false, false, &[]), None);

        let vertical = crate::layout::SplitBorder {
            pos: 9,
            direction: ratatui::layout::Direction::Vertical,
            ratio: 0.5,
            area: Rect::new(2, 3, 40, 12),
            path: vec![true],
        };
        assert_eq!(
            split_hit_rect(&vertical, true, true, &[]),
            Some(Rect::new(2, 8, 40, 2))
        );

        let edge = crate::layout::SplitBorder {
            pos: 0,
            direction: ratatui::layout::Direction::Horizontal,
            ratio: 0.5,
            area: Rect::new(0, 0, 1, 4),
            path: Vec::new(),
        };
        assert_eq!(
            split_hit_rect(&edge, true, true, &[]),
            Some(Rect::new(0, 0, 1, 4))
        );
        assert_eq!(split_hit_rect(&edge, false, true, &[]), None);
        assert_eq!(
            split_hit_rect(&horizontal, false, true, &[Rect::new(19, 3, 1, 12)]),
            None
        );
    }
}
