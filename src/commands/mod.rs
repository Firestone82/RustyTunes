pub mod cmd_help;
pub mod cmd_play;
pub mod cmd_skip;
pub mod cmd_stop;
pub mod cmd_vol;
pub mod cmd_queue;
pub mod cmd_join;
pub mod cmd_leave;
pub mod cmd_shuffle;
pub mod cmd_playing;

#[cfg(any(target_os = "windows"))]
pub mod cmd_uwu;

pub mod cmd_notify;
pub mod cmd_wakeup;