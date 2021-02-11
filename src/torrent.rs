use lava_torrent::torrent::v1::Torrent;

struct Torrent {
    /// if we seed or not
    seeding: bool,
    info_hash: String, //or in bytes with vec<u8> ?
    //piece, size, ...
}