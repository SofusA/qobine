let evtSource;

function initSse() {
  if (evtSource) {
    evtSource.close();
  }
  evtSource = new EventSource("/sse");

  evtSource.addEventListener("reload", () => {
    console.warn("Reload event");
    location.reload();
  });

  evtSource.addEventListener("status", () => {
    for (const el of document.querySelectorAll("[data-sse=status]")) {
      htmx.trigger(el, "status");
    }
  });

  evtSource.addEventListener("autoplay", () => {
    console.warn("auto play");
    for (const el of document.querySelectorAll("[data-sse=autoplay]")) {
      htmx.trigger(el, "autoplay");
    }
  });

  evtSource.addEventListener("tracklist", () => {
    for (const el of document.querySelectorAll("[data-sse=tracklist]")) {
      htmx.trigger(el, "tracklist");
    }
  });

  evtSource.addEventListener("volume", (event) => {
    const slider = document.getElementById("volume-slider");
    if (slider) {
      slider.value = event.data;
    }

    const percentage = document.getElementById("volume-percentage");
    if (percentage) {
      percentage.innerHTML = `${event.data}%`;
    }
  });

  for (const level of ["error", "warn", "success", "info"]) {
    evtSource.addEventListener(level, (event) => {
      htmx.swap("#toast-container", event.data, { swapStyle: "afterbegin" });
    });
  }

  evtSource.addEventListener("position", (event) => {
    const slider = document.getElementById("progress-slider");
    if (!slider) return;
    slider.value = event.data;

    const positionElement = document.getElementById("position");
    if (!positionElement) return;

    const totalSeconds = Math.floor(event.data / 1000);
    const minutes = String(Math.floor(totalSeconds / 60)).padStart(2, "0");
    const seconds = String(totalSeconds % 60).padStart(2, "0");
    positionElement.textContent = `${minutes}:${seconds}`;
  });

  evtSource.addEventListener("available-devices", () => {
    for (const el of document.querySelectorAll(
      "[data-sse~=available-devices]",
    )) {
      htmx.trigger(el, "available-devices");
    }
  });

  evtSource.addEventListener("active-device", () => {
    console.warn("new active device");
    for (const el of document.querySelectorAll("[data-sse~=active-device]")) {
      htmx.trigger(el, "active-device");
    }
  });
}

initSse();

function refreshSse() {
  for (const el of document.querySelectorAll("[hx-trigger='tracklist']")) {
    htmx.trigger(el, "tracklist");
  }
  for (const el of document.querySelectorAll("[hx-trigger='status']")) {
    htmx.trigger(el, "status");
  }
}

document.addEventListener("visibilitychange", () => {
  if (!document.hidden) {
    initSse();
    refreshSse();
  }
});

function focusSearchInput() {
  const input = document.getElementById("query");
  if (input) input.focus();
}

function updateSearchState(value) {
  sessionStorage.setItem("search-query", value);

  const url = new URL(window.location.href);
  if (value && value.trim() !== "") {
    url.searchParams.set("query", value);
  } else {
    url.searchParams.delete("query");
  }
  history.replaceState(null, "", url.toString());

  const encoded = encodeURIComponent(value);
  for (const id of [
    "albums-tab",
    "artists-tab",
    "playlists-tab",
    "tracks-tab",
  ]) {
    const tab = document.getElementById(id);
    if (tab) {
      const section = id.replace("-tab", "");
      tab.href = `${section}?query=${encoded}`;
    }
  }
}

function loadSearchInput() {
  const value = sessionStorage.getItem("search-query");
  if (value) {
    const input = document.getElementById("query");
    if (input) input.value = value;
    updateSearchState(value);
  }
}

let searchTimeout;

function setSearchQuery(value) {
  clearTimeout(searchTimeout);
  searchTimeout = setTimeout(() => updateSearchState(value), 500);
}

function initSortable(root = document) {
  const sortables = [];

  if (root.matches?.(".sortable")) {
    sortables.push(root);
  }

  sortables.push(...root.querySelectorAll(".sortable"));

  for (const sortable of sortables) {
    if (Sortable.get(sortable)) {
      continue;
    }

    new Sortable(sortable, {
      animation: 150,
      handle: ".handle",
    });
  }
}

htmx.onLoad(function (content) {
  initSortable(content);
});
