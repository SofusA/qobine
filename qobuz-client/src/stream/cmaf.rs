use crate::Error;

const QBZ_INIT_UUID: [u8; 16] = [
    0xc7, 0xc7, 0x5d, 0xf0, 0xfd, 0xd9, 0x51, 0xe9, 0x8f, 0xc2, 0x29, 0x71, 0xe4, 0xac, 0xf8, 0xd2,
];
const QBZ_SEGMENT_UUID: [u8; 16] = [
    0x3b, 0x42, 0x12, 0x92, 0x56, 0xf3, 0x5f, 0x75, 0x92, 0x36, 0x63, 0xb6, 0x9a, 0x1f, 0x52, 0xb2,
];

/// Info about one segment from the init segment's segment table.
#[derive(Debug, Clone)]
pub struct SegmentTableEntry {
    /// Byte size of this segment's decrypted FLAC frame data.
    pub byte_len: u32,
    /// Number of audio samples in this segment (useful for future timestamp-based seeking).
    #[allow(dead_code)]
    pub sample_count: u32,
}

/// FLAC header and segment table extracted from the init segment.
pub struct InitInfo {
    pub flac_header: Vec<u8>,
    /// Per-segment sizes (indices 0..n_segments-1 correspond to segments `1..n_segments`).
    pub segment_table: Vec<SegmentTableEntry>,
}

/// One frame entry from the segment's `QBZ_SEGMENT_UUID` box.
pub struct FrameEntry {
    pub size: u32,
    pub flags: u16,
    pub iv: [u8; 8],
}

/// Parsed crypto info from a segment's `QBZ_SEGMENT_UUID` box.
pub struct SegmentCrypto {
    /// Offset to the start of audio frame data (usually mdat payload).
    pub data_offset: usize,
    /// End of the mdat box content. Data between the last frame entry and this
    /// offset is unencrypted trailing audio that must be included in output.
    pub mdat_end: usize,
    pub entries: Vec<FrameEntry>,
}

/// Parse the init segment (segment 0) to extract the FLAC header.
pub fn parse_init_segment(data: &[u8]) -> Result<InitInfo, Error> {
    let mut pos: usize = 0;

    while let Some(pos_plus_8) = pos.checked_add(8) {
        let Some(pos_plus_4) = pos.checked_add(4) else {
            break;
        };

        let Some(box_header) = data.get(pos_plus_4..pos_plus_8) else {
            break;
        };

        let size = read_box_size(data, pos);

        let Some(end) = pos.checked_add(size) else {
            break;
        };

        if size < 8 || end > data.len() {
            break;
        }

        if box_header == b"uuid" {
            let Some(posision_plus_24) = pos.checked_add(24) else {
                break;
            };

            let Some(uuid) = data.get(pos_plus_8..posision_plus_24) else {
                break;
            };

            if uuid == QBZ_INIT_UUID {
                let Some(payload) = data.get(posision_plus_24..end) else {
                    break;
                };

                return parse_init_uuid_payload(payload);
            }
        }

        pos = end;
    }

    Err(Error::Stream {
        message: "init segment: QBZ_INIT_UUID box not found".into(),
    })
}

/// Parse an audio segment to extract per-frame crypto info.
pub fn parse_segment_crypto(data: &[u8]) -> Result<SegmentCrypto, Error> {
    let mut uuid_pos = None;
    let mut mdat_end = data.len();

    let mut pos: usize = 0;
    while let Some(pos_plus_8) = pos.checked_add(8) {
        if pos_plus_8 > data.len() {
            break;
        }

        let size = read_box_size(data, pos);

        let Some(end) = pos.checked_add(size) else {
            break;
        };

        if size < 8 || end > data.len() {
            break;
        }

        let Some(pos_plus_4) = pos.checked_add(4) else {
            break;
        };

        let Some(box_type) = data.get(pos_plus_4..pos_plus_8) else {
            break;
        };

        if box_type == b"uuid" {
            let Some(posision_plus_24) = pos.checked_add(24) else {
                break;
            };

            if let Some(uuid) = data.get(pos_plus_8..posision_plus_24)
                && uuid == QBZ_SEGMENT_UUID
            {
                uuid_pos = Some(pos);
            }
        } else if box_type == b"mdat" {
            mdat_end = end;
        }

        pos = end;
    }

    match uuid_pos {
        Some(p) => parse_segment_uuid_payload(data, p, mdat_end),
        None => Err(Error::Stream {
            message: "audio segment: QBZ_SEGMENT_UUID box not found".into(),
        }),
    }
}

// --- Internal helpers ---
struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn take(&mut self, len: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(len)?;
        let slice = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(slice)
    }

    fn skip(&mut self, len: usize) -> Option<()> {
        self.take(len)?;
        Some(())
    }

    fn take_u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    fn take_u16(&mut self) -> Option<u16> {
        let b: [u8; 2] = self.take(2)?.try_into().ok()?;
        Some(u16::from_be_bytes(b))
    }

    fn take_u32(&mut self) -> Option<u32> {
        let b: [u8; 4] = self.take(4)?.try_into().ok()?;
        Some(u32::from_be_bytes(b))
    }

    fn take_u24(&mut self) -> Option<u32> {
        let b: [u8; 3] = self.take(3)?.try_into().ok()?;
        Some((u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]))
    }
}

fn parse_init_uuid_payload(payload: &[u8]) -> Result<InitInfo, Error> {
    // The init UUID payload layout (from JS function d()):
    //   [4B padding/version]
    //   [4B track_id]
    //   [4B file_id]
    //   [4B sample_rate]
    //   [1B bits_per_sample]
    //   [1B channels + 2B padding]
    //   [6B total_samples_count]
    //   [2B initial_data_raw_len]
    //   [initial_data_raw_len bytes: FLAC header data]
    //   [1B key_id_len]
    //   [key_id_len bytes: key_id]
    //   [2B segment_count]
    //   Per segment: [4B byte_len][4B sample_count]

    let mut c = Cursor::new(payload);

    c.skip(26).ok_or_else(|| Error::Stream {
        message: "init UUID payload too short".into(),
    })?;

    let raw_len = usize::from(c.take_u16().ok_or_else(|| Error::Stream {
        message: "init UUID payload truncated at raw_len".into(),
    })?);

    let raw_data = c
        .take(raw_len)
        .unwrap_or_else(|| payload.get(c.pos..).unwrap_or_default());

    let flac_pos = raw_data
        .windows(4)
        .position(|w| w == b"fLaC")
        .ok_or_else(|| Error::Stream {
            message: "init UUID payload: fLaC magic not found".into(),
        })?;

    let header_len = 42;
    let flac_end = flac_pos
        .checked_add(header_len)
        .ok_or_else(|| Error::Stream {
            message: "init UUID payload: STREAMINFO truncated".into(),
        })?;

    let flac_slice = raw_data
        .get(flac_pos..flac_end)
        .ok_or_else(|| Error::Stream {
            message: "init UUID payload: STREAMINFO truncated".into(),
        })?;

    let mut flac_header = flac_slice.to_vec();
    let flag = flac_header.get_mut(4).ok_or_else(|| Error::Stream {
        message: "init UUID payload: invalid STREAMINFO header".into(),
    })?;
    *flag |= 0x80;

    let Some(key_id_len) = c.take_u8().map(usize::from) else {
        return Ok(InitInfo {
            flac_header,
            segment_table: Vec::new(),
        });
    };

    c.skip(key_id_len);

    let mut segment_table = Vec::new();

    if let Some(seg_count) = c.take_u16().map(usize::from) {
        for _ in 0..seg_count {
            let Some(byte_len) = c.take_u32() else {
                break;
            };
            let Some(sample_count) = c.take_u32() else {
                break;
            };

            segment_table.push(SegmentTableEntry {
                byte_len,
                sample_count,
            });
        }
    }

    Ok(InitInfo {
        flac_header,
        segment_table,
    })
}

fn parse_segment_uuid_payload(
    data: &[u8],
    uuid_box_start: usize,
    mdat_end: usize,
) -> Result<SegmentCrypto, Error> {
    // Layout after box header (8) + UUID (16) = offset 24:
    //   [4B version/padding]
    //   [4B data_offset]    — offset from uuid_box_start to audio data
    //   [1B iv_size]
    //   [3B frame_count]
    //   Per frame (16 bytes): [4B size][2B skip][2B flags][8B iv]

    let base = uuid_box_start
        .checked_add(24)
        .ok_or_else(|| Error::Stream {
            message: "segment UUID payload offset overflow".into(),
        })?;

    let payload = data.get(base..).ok_or_else(|| Error::Stream {
        message: "segment UUID payload too short for header".into(),
    })?;

    let mut c = Cursor::new(payload);

    c.skip(4).ok_or_else(|| Error::Stream {
        message: "segment UUID payload too short for header".into(),
    })?;

    let data_offset_raw = c.take_u32().ok_or_else(|| Error::Stream {
        message: "segment UUID payload too short for data_offset".into(),
    })?;

    let data_offset = uuid_box_start
        .checked_add(usize::try_from(data_offset_raw).map_err(|_| Error::Stream {
            message: "segment UUID data_offset overflow".into(),
        })?)
        .ok_or_else(|| Error::Stream {
            message: "segment UUID data_offset overflow".into(),
        })?;

    let iv_size = usize::from(c.take_u8().ok_or_else(|| Error::Stream {
        message: "segment UUID payload too short for iv_size".into(),
    })?);

    let frame_count = usize::try_from(c.take_u24().ok_or_else(|| Error::Stream {
        message: "segment UUID payload too short for frame_count".into(),
    })?)
    .map_err(|_| Error::Stream {
        message: "segment UUID invalid frame_count".into(),
    })?;

    let entry_size = 8usize.checked_add(iv_size).ok_or_else(|| Error::Stream {
        message: "segment UUID entry size overflow".into(),
    })?;

    let entries_bytes = frame_count
        .checked_mul(entry_size)
        .ok_or_else(|| Error::Stream {
            message: "segment UUID entry table overflow".into(),
        })?;

    if c.pos
        .checked_add(entries_bytes)
        .is_none_or(|end| end > payload.len())
    {
        return Err(Error::Stream {
            message: format!(
                "segment UUID: not enough data for {frame_count} entries of {entry_size} bytes"
            ),
        });
    }

    let mut entries = Vec::with_capacity(frame_count);

    for _ in 0..frame_count {
        let size = c.take_u32().ok_or_else(|| Error::Stream {
            message: "segment UUID truncated entry".into(),
        })?;

        c.skip(2).ok_or_else(|| Error::Stream {
            message: "segment UUID truncated entry".into(),
        })?;

        let flags = c.take_u16().ok_or_else(|| Error::Stream {
            message: "segment UUID truncated entry".into(),
        })?;

        let mut iv = [0u8; 8];

        let iv_bytes = c.take(iv_size).ok_or_else(|| Error::Stream {
            message: "segment UUID truncated entry".into(),
        })?;

        let copy_len = iv_bytes.len().min(iv.len());

        for (dst, src) in iv.iter_mut().zip(iv_bytes.iter()).take(copy_len) {
            *dst = *src;
        }

        entries.push(FrameEntry { size, flags, iv });
    }

    Ok(SegmentCrypto {
        data_offset,
        mdat_end,
        entries,
    })
}

fn read_box_size(data: &[u8], pos: usize) -> usize {
    let Some(bytes) = data.get(pos..) else {
        return 0;
    };

    let Some(header) = bytes.get(..4) else {
        return 0;
    };

    let Ok(header): Result<[u8; 4], _> = header.try_into() else {
        return 0;
    };

    let s = u32::from_be_bytes(header);

    match s {
        0 => data.len().saturating_sub(pos),
        2..=7 => 0,
        n => usize::try_from(n).unwrap_or(0),
    }
}
