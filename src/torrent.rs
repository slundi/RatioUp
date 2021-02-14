use lava_torrent::torrent::v1::Torrent;

struct ConfiguredTorrent {
    torrent: Torrent,
    /// if we seed or not
    started: bool,
    info_hash: String, //or in bytes with vec<u8> ?
    //piece, size, ...
}