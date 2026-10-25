//! Team radio voice: when a line goes on screen its pre-generated clip
//! (`assets/radio/audio/<clip>.ogg`, `just voices`) plays; when the line
//! ends, is cut or dropped (or the voice is turned off) the clip stops. The
//! radio picks the clip (`radio::clip_name`); a line without one is text
//! only. Volume: the `voice_volume` setting.

use bevy::audio::{AudioPlayer, AudioSink, AudioSinkPlayback, PlaybackSettings, Volume};
use bevy::prelude::*;

use crate::GameSet;
use crate::radio::Radio;
use crate::settings::Settings;

pub struct RadioAudioPlugin;

impl Plugin for RadioAudioPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<VoicePlayback>()
            .add_systems(Update, play_voice.after(GameSet::Sim));
    }
}

/// The clip playing: the radio line's id and the entity of its player.
#[derive(Resource, Default)]
struct VoicePlayback {
    playing: Option<(u64, String, Entity)>,
}

/// Starts the clip of a new line, stops the clip of a line that is gone or
/// lost its clip, follows the volume setting.
fn play_voice(
    mut commands: Commands,
    radio: Res<Radio>,
    settings: Res<Settings>,
    assets: Res<AssetServer>,
    mut playback: ResMut<VoicePlayback>,
    mut sinks: Query<&mut AudioSink>,
) {
    let wanted = radio.current().and_then(|c| c.clip.as_ref().map(|clip| (c.id, clip)));
    let same = match (&playback.playing, wanted) {
        (Some((id, clip, _)), Some((wid, wclip))) => *id == wid && clip == wclip,
        (None, None) => true,
        _ => false,
    };
    let volume = Volume::Linear(settings.voice_volume);
    if same {
        if let Some((_, _, e)) = &playback.playing
            && let Ok(mut sink) = sinks.get_mut(*e)
            && sink.volume() != volume
        {
            sink.set_volume(volume);
        }
        return;
    }
    if let Some((_, _, e)) = playback.playing.take() {
        if let Ok(sink) = sinks.get(e) {
            sink.stop();
        }
        if let Ok(mut entity) = commands.get_entity(e) {
            entity.try_despawn();
        }
    }
    if let Some((id, clip)) = wanted {
        let source = assets.load(format!("radio/audio/{clip}.ogg"));
        let e = commands
            .spawn((AudioPlayer::new(source), PlaybackSettings::DESPAWN.with_volume(volume)))
            .id();
        playback.playing = Some((id, clip.clone(), e));
    }
}
