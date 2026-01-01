const { Component, mount, xml, useEffect, useState } = owl;

class App extends Component {
  setup() {
    this.state = useState({
      torrents: [],
    });

    useEffect(
      () => {
        const fetchTorrents = async () => {
          try {
            const response = await fetch("/api");
            if (response.ok) {
              const data = await response.json();
              this.state.torrents = data;
              console.log("Fetched torrents:", this.state.torrents);
            } else {
              console.error("Failed to fetch torrents:", response.statusText);
            }
          } catch (error) {
            console.error("Error fetching torrents:", error);
          }
        };

        fetchTorrents();

        const interval = setInterval(fetchTorrents, 1000); // Refresh every 2 seconds
        return () => clearInterval(interval);
      },
      () => []
    );
  }

  byteToHumanReadable = (bytes) => {
    const sizes = ["B", "KB", "MB", "GB", "TB"];
    if (bytes === 0) {
      return "0 B";
    }

    const i = Math.floor(Math.log(bytes) / Math.log(1024));
    return parseFloat((bytes / Math.pow(1024, i)).toFixed(2)) + " " + sizes[i];
  };

  secondToHumanReadable = (seconds) => {
    if (seconds === 0) {
      return "00:00:00";
    }

    const hrs = Math.floor(seconds / 3600);
    const mins = Math.floor((seconds % 3600) / 60);
    const secs = Math.floor(seconds % 60);
    const pad = (n) => String(n).padStart(2, "0");
    return `${pad(hrs)}:${pad(mins)}:${pad(secs)}`;
  };

  static template = xml`
    <div class="w-100 h-100 overflow-hidden">
      <h1 class="text-center">Torrent List</h1>
      <div class="w-100 d-flex flex-column p-2">
        <div t-foreach="this.state.torrents"
          t-as="torrent"
          t-key="torrent_index"
          class="border rounded w-100 p-3 mb-2">
          <p t-esc="torrent.name" class="fs-5 p-0 m-0" />
          <p t-esc="byteToHumanReadable(torrent.length)" class="p-0 m-0 fs-7" />
          <div class="d-flex align-items-center gap-1 mt-2 w-100">
            <span class="badge text-bg-success">
              <span t-esc="torrent.seeders"/> Seeds
            </span>
            <span class="badge text-bg-danger">
              <span t-esc="torrent.leechers"/> Leeches
            </span>
            <span class="badge bg-secondary">
              Last announce <span t-esc="secondToHumanReadable(torrent.last_announce_sec)"/>
            </span>
            <span class="badge bg-secondary ms-auto">
              Next announce <span t-esc="secondToHumanReadable(torrent.interval - torrent.last_announce_sec)"/>
            </span>
          </div>
        </div>
      </div>
    </div>
  `;
}

mount(App, document.body);
