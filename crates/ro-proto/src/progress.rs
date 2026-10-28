//! Achievements, titles and attendance, as rAthena sends them at PACKETVER 20250402.

use bytes::{Buf, BufMut, BytesMut};

use crate::codec::Encode;

/// One achievement's progress (ach_list_info, 50 bytes).
#[derive(Debug, Clone, PartialEq)]
pub struct Achievement {
    pub id: u32,
    pub completed: bool,
    pub objectives: [u32; 10],
    /// Unix time it was completed (0 if not).
    pub completed_at: u32,
    /// The reward was collected.
    pub rewarded: bool,
}

/// Achievement totals: (total points, rank, points into this rank, points for the next).
pub type AchRank = (u32, u16, u32, u32);

fn achievement(b: &mut &[u8]) -> Achievement {
    let id = b.get_u32_le();
    let completed = b.get_u8() != 0;
    let mut objectives = [0; 10];
    for o in &mut objectives {
        *o = b.get_u32_le();
    }
    let completed_at = b.get_u32_le();
    let rewarded = b.get_u8() != 0;
    Achievement { id, completed, objectives, completed_at, rewarded }
}

/// ZC_ALL_ACH_LIST (0x0a23): every achievement with progress, and the totals.
pub fn parse_achievements(data: &[u8]) -> Option<(AchRank, Vec<Achievement>)> {
    let mut b = data.get(4..)?;
    if b.len() < 18 {
        return None;
    }
    b.advance(4); // total achievements
    let rank = (b.get_u32_le(), b.get_u16_le(), b.get_u32_le(), b.get_u32_le());
    let mut list = vec![];
    while b.len() >= 50 {
        list.push(achievement(&mut b));
    }
    Some((rank, list))
}

/// ZC_ACH_UPDATE (0x0a24): one achievement changed.
pub fn parse_achievement_update(data: &[u8]) -> Option<(AchRank, Achievement)> {
    let mut b = data.get(2..66)?;
    let rank = (b.get_u32_le(), b.get_u16_le(), b.get_u32_le(), b.get_u32_le());
    Some((rank, achievement(&mut b)))
}

/// ZC_REQ_ACH_REWARD_ACK (0x0a26): (failed, achievement id).
pub fn parse_reward_ack(data: &[u8]) -> Option<(bool, u32)> {
    let mut b = data.get(2..7)?;
    Some((b.get_u8() != 0, b.get_u32_le()))
}

/// ZC_ACK_CHANGE_TITLE (0x0a2f): (failed, title id).
pub fn parse_title_ack(data: &[u8]) -> Option<(bool, u32)> {
    let mut b = data.get(2..7)?;
    Some((b.get_u8() != 0, b.get_u32_le()))
}

/// ZC_UI_OPEN (0x0ae2): (ui type, data). Type 7 is attendance, data = days * 10 + claimed today.
pub fn parse_ui_open(data: &[u8]) -> Option<(u8, i32)> {
    let mut b = data.get(2..7)?;
    Some((b.get_u8(), b.get_i32_le()))
}

/// ZC_CHECK_ATTENDANCE (0x0af0): the counter after claiming (days * 10 + claimed today).
pub fn parse_attendance(data: &[u8]) -> Option<i32> {
    data.get(6..10).map(|mut b| b.get_i32_le())
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProgressRequest {
    /// CZ_REQ_ACH_REWARD.
    Reward(u32),
    /// CZ_REQ_CHANGE_TITLE (0 removes the title).
    Title(u32),
    /// CZ_OPEN_UI with the attendance type.
    OpenAttendance,
    /// CZ_REQ_ATTENDANCE_REWARD: claim today's reward.
    ClaimAttendance,
}

impl Encode for ProgressRequest {
    fn encode(&self, d: &mut BytesMut) {
        match *self {
            ProgressRequest::Reward(id) => {
                d.put_u16_le(0x0a25);
                d.put_u32_le(id);
            }
            ProgressRequest::Title(id) => {
                d.put_u16_le(0x0a2e);
                d.put_u32_le(id);
            }
            ProgressRequest::OpenAttendance => {
                d.put_u16_le(0x0a68);
                d.put_u8(5);
            }
            ProgressRequest::ClaimAttendance => d.put_u16_le(0x0aef),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_update() {
        let mut d = vec![0x24, 0x0a];
        d.extend(30u32.to_le_bytes());
        d.extend(2u16.to_le_bytes());
        d.extend(5u32.to_le_bytes());
        d.extend(50u32.to_le_bytes());
        d.extend(120001u32.to_le_bytes());
        d.push(1);
        d.extend([0; 40]);
        d.extend(7u32.to_le_bytes());
        d.push(0);
        let (rank, a) = parse_achievement_update(&d).unwrap();
        assert_eq!(rank, (30, 2, 5, 50));
        assert_eq!(a.id, 120001);
        assert!(a.completed && !a.rewarded);
    }
}
