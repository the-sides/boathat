//! Panel task ownership after a fleet change, diagnostics and loop shutdown.

use super::dims::AgentDims;
use super::event_loop::{dump_state_tier, refit, restart_all_agents, LoopOutcome};
use super::tasks::Tasks;
use crate::app::{App, Msg};
use crate::diag::Phase;
use std::sync::Arc;
use tokio::sync::{mpsc::Sender, watch};

pub(super) fn finish_loop(
    app: &mut App,
    tasks: &mut Tasks,
    error: Option<std::io::Error>,
) -> LoopOutcome {
    let killed = app.running_upgrade_hosts();
    tasks.abort_all(app);
    LoopOutcome { killed, error }
}

type LoopChannels<'a> = (&'a Arc<watch::Receiver<(u16, u16)>>, &'a Sender<Msg>);

/// Reconcile task ownership after any input, then publish loop diagnostics.
pub(super) fn settle_iteration<B>(
    terminal: &ratatui::Terminal<B>,
    dims: &mut AgentDims,
    app: &mut App,
    tasks: &mut Tasks,
    channels: LoopChannels<'_>,
    known_epoch: &mut u64,
    diag: &crate::diag::Diag,
) where
    B: ratatui::backend::Backend,
{
    let (dims_rx, tx) = channels;
    if *known_epoch != app.panels_epoch {
        tasks.abort_all(app);
        *known_epoch = app.panels_epoch;
        tasks.fit_to(app.panels.len());
        refit(terminal, dims, app);
        restart_all_agents(app, dims_rx, tx, tasks);
    }
    diag.bump_iter();
    if let Some((seq, sig)) = diag.take_request() {
        dump_state_tier(diag, app, tasks, seq, sig);
    }
    diag.set_phase(Phase::Idle);
}
