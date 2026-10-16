//! Pikmin/captain motions (`PIKIANIM_*`, PaniPikiAnimator.h) and the table mapping them to
//! AnimInfo indices in `pikis/animMgr.bin` (`PaniPikiAnimator::createMotionTable`,
//! plugPikiKando/panipikianimator.cpp). Pikmin and Olimar share both.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Motion {
    Run = 0,
    Nigeru = 1,
    Walk = 2,
    Wait = 3,
    Pick = 4,
    Nuku = 5,
    Nukareru = 6,
    Dead = 7,
    Dead2 = 8,
    Dead3 = 9,
    Damage = 10,
    Asibumi = 11,
    /// NaviGeyzerState::init's airborne pose (naviState.cpp).
    OCarry = 12,
    Tanemaki = 15,
    GrowUp1 = 17,
    GrowUp2 = 18,
    Korobu = 20,
    Jump = 21,
    StillJump = 22,
    Attack = 23,
    Punch = 25,
    Throw = 27,
    Hang = 28,
    Fall = 29,
    JKoke = 30,
    JHit = 31,
    GetUp = 32,
    RollJmp = 34,
    WaveJmp = 35,
    ThrowWait = 36,
    Akubi = 39,
    PickLoop = 46,
    Mizunomi = 47,
    Kuttuku = 48,
    Fue = 49,
    Press1 = 53,
    SPress = 55,
    HNoboru = 62,
    Noboru = 63,
    /// PikiEmotionState Happy and NaviClearState, from PaniPikiAnimator.h.
    RotJump = 75,
    NukuFast = 82,
    NukareFast = 83,
    ODead = 89,
}

/// `createMotionTable`: motion id → AnimInfo index (`None` for id 68, which it never sets).
pub const MOTION_TABLE: [Option<u8>; 90] = {
    let pairs: [(usize, u8); 89] = [
        (0, 0), (1, 4), (2, 25), (3, 12), (4, 1), (5, 28), (6, 29), (7, 20), (8, 42), (9, 48),
        (10, 21), (11, 44), (12, 45), (13, 52), (14, 53), (15, 34), (16, 35), (17, 39), (18, 40),
        (19, 41), (20, 24), (21, 9), (22, 13), (23, 18), (24, 33), (25, 94), (26, 36), (27, 54),
        (28, 55), (29, 56), (30, 57), (31, 58), (32, 59), (33, 60), (34, 61), (35, 62), (36, 63),
        (37, 64), (38, 31), (39, 10), (40, 16), (41, 3), (42, 38), (43, 47), (44, 32), (45, 66),
        (46, 68), (47, 67), (48, 71), (49, 74), (50, 75), (51, 76), (52, 77), (53, 78), (54, 79),
        (55, 80), (56, 81), (57, 82), (58, 83), (59, 84), (60, 85), (61, 86), (62, 87), (63, 88),
        (64, 89), (65, 90), (66, 91), (67, 92), (69, 95), (70, 8), (71, 26), (72, 5), (73, 6),
        (74, 7), (75, 14), (76, 11), (77, 17), (78, 19), (79, 27), (80, 37), (81, 22), (82, 43),
        (83, 93), (84, 69), (85, 72), (86, 96), (87, 97), (88, 98), (89, 99),
    ];
    let mut t = [None; 90];
    let mut i = 0;
    while i < pairs.len() {
        t[pairs[i].0] = Some(pairs[i].1);
        i += 1;
    }
    t
};

impl Motion {
    pub fn anim_index(self) -> Option<usize> {
        MOTION_TABLE[self as usize].map(usize::from)
    }
}
