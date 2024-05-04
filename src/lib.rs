// The point is to distribute ids across logical volume shards evenly
//     - reset sequence every MS to 0 to remain sortable
//     - increase logical volume sequence by 1 every MS
//     - return err if available logical volume ids have been used

// This assumes sequences + logical volume ids occur in the same ms
// https://instagram-engineering.com/sharding-ids-at-instagram-1cf5a71e5a5c

use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SEQUENCE_BIT_LEN: u64 = 10;
const SEQUENCE_BIT_MASK: u64 = (1 << SEQUENCE_BIT_LEN) - 1;
const MAX_SEQUENCES: u64 = u32::pow(2, SEQUENCE_BIT_LEN as u32) as u64;
const LOGICAL_VOLUME_BIT_LEN: u64 = 13;
const LOGICAL_VOLUME_BIT_MASK: u64 = ((1 << LOGICAL_VOLUME_BIT_LEN) - 1) << SEQUENCE_BIT_LEN;
const MAX_LOGICAL_VOLUMES: u64 = u32::pow(2, LOGICAL_VOLUME_BIT_LEN as u32) as u64;

#[derive(Debug)]
pub enum Error {
    LogicalVolumeModuleIsZero,
    ExceededAvailableLogicalVolumes,
    FailedToParseOriginDuration,
    NoAvailableSequences,
}

pub struct Snowprint {
    settings: SnowprintSettings,
    state: SnowprintState,
}

pub struct SnowprintSettings {
    pub origin_duration: Duration,
    pub logical_volume_modulo: u64,
    pub logical_volume_base: u64,
}

struct SnowprintState {
    last_duration_ms: u64,
    sequence_id: u64,
    logical_volume_id: u64,
    last_logical_volume_id: u64,
}

impl Snowprint {
    pub fn new(settings: SnowprintSettings) -> Result<Snowprint, Error> {
        if let Err(err) = check_settings(&settings) {
            return Err(err);
        }

        let duration_ms =
            match SystemTime::now().duration_since(UNIX_EPOCH + settings.origin_duration) {
                Ok(duration) => duration.as_millis() as u64,
                _ => return Err(Error::FailedToParseOriginDuration),
            };

        Ok(Snowprint {
            settings: settings,
            state: SnowprintState {
                last_duration_ms: duration_ms,
                sequence_id: 0,
                logical_volume_id: 0,
                last_logical_volume_id: 0,
            },
        })
    }

    pub fn get_snowprint(&mut self) -> Result<u64, Error> {
        let duration_ms =
            get_most_recent_duration(self.settings.origin_duration, self.state.last_duration_ms);
        compose_snowprint_from_settings_and_state(&self.settings, &mut self.state, duration_ms)
    }
}

fn check_settings(settings: &SnowprintSettings) -> Result<(), Error> {
    if settings.logical_volume_modulo == 0 {
        return Err(Error::LogicalVolumeModuleIsZero);
    }
    if (settings.logical_volume_base + settings.logical_volume_modulo) > MAX_LOGICAL_VOLUMES {
        return Err(Error::ExceededAvailableLogicalVolumes);
    }

    Ok(())
}

fn get_most_recent_duration(origin_duration: Duration, last_duration_ms: u64) -> u64 {
    match SystemTime::now().duration_since(UNIX_EPOCH + origin_duration) {
        // check time didn't go backward
        Ok(duration) => {
            let dur_ms = duration.as_millis() as u64;
            match dur_ms > last_duration_ms {
                true => dur_ms,
                _ => last_duration_ms,
            }
        }
        // yikes! time went backwards so use the most recent step
        _ => last_duration_ms,
    }
}

fn compose_snowprint_from_settings_and_state(
    settings: &SnowprintSettings,
    state: &mut SnowprintState,
    duration_ms: u64,
) -> Result<u64, Error> {
    // the following will mutate state object
    match state.last_duration_ms < duration_ms {
        true => time_changed(state, settings, duration_ms),
        _ => {
            if let Err(err) = time_did_not_change(state, settings) {
                return Err(err);
            };
        }
    }

    Ok(compose_snowprint(
        duration_ms,
        settings.logical_volume_base + state.logical_volume_id,
        state.sequence_id,
    ))
}

fn time_changed(state: &mut SnowprintState, settings: &SnowprintSettings, duration_ms: u64) {
    state.last_duration_ms = duration_ms;
    state.sequence_id = 0;
    state.last_logical_volume_id = state.logical_volume_id;
    state.logical_volume_id = (state.logical_volume_id + 1) % settings.logical_volume_modulo;
}

fn time_did_not_change(
    state: &mut SnowprintState,
    settings: &SnowprintSettings,
) -> Result<(), Error> {
    state.sequence_id += 1;
    if state.sequence_id > MAX_SEQUENCES - 1 {
        let next_logical_volume_id = (state.logical_volume_id + 1) % settings.logical_volume_modulo;
        // cycled through all sequences on all available logical shards
        if next_logical_volume_id == state.last_logical_volume_id {
            return Err(Error::NoAvailableSequences);
        }
        // move to next shard
        state.sequence_id = 0;
        state.logical_volume_id = next_logical_volume_id;
    }
    Ok(())
}

// at it's core this is a snowprint
pub fn compose_snowprint(ms_timestamp: u64, logical_id: u64, ticket_id: u64) -> u64 {
    ms_timestamp << (LOGICAL_VOLUME_BIT_LEN + SEQUENCE_BIT_LEN)
        | logical_id << SEQUENCE_BIT_LEN
        | ticket_id
}

pub fn decompose_snowprint(snowprint: u64) -> (u64, u64, u64) {
    let time = snowprint >> (LOGICAL_VOLUME_BIT_LEN + SEQUENCE_BIT_LEN);
    let logical_id = (snowprint & LOGICAL_VOLUME_BIT_MASK) >> SEQUENCE_BIT_LEN;
    let ticket_id = snowprint & SEQUENCE_BIT_MASK;

    (time, logical_id, ticket_id)
}
