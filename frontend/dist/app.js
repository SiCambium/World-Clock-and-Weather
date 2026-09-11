(function () {
  "use strict";

  const MIN_PLACES = 2;
  const MAX_PLACES = 10;

  // WMO weather codes -> icon + label
  // https://open-meteo.com/en/docs (weather_code)
  const WEATHER_CODES = {
    0: ["☀️", "Clear sky"],
    1: ["🌤️", "Mostly clear"],
    2: ["⛅", "Partly cloudy"],
    3: ["☁️", "Overcast"],
    45: ["🌫️", "Fog"],
    48: ["🌫️", "Rime fog"],
    51: ["🌦️", "Light drizzle"],
    53: ["🌦️", "Drizzle"],
    55: ["🌧️", "Dense drizzle"],
    56: ["🌧️", "Freezing drizzle"],
    57: ["🌧️", "Freezing drizzle"],
    61: ["🌦️", "Light rain"],
    63: ["🌧️", "Rain"],
    65: ["🌧️", "Heavy rain"],
    66: ["🌧️", "Freezing rain"],
    67: ["🌧️", "Freezing rain"],
    71: ["🌨️", "Light snow"],
    73: ["🌨️", "Snow"],
    75: ["❄️", "Heavy snow"],
    77: ["❄️", "Snow grains"],
    80: ["🌦️", "Light showers"],
    81: ["🌧️", "Showers"],
    82: ["⛈️", "Violent showers"],
    85: ["🌨️", "Snow showers"],
    86: ["❄️", "Heavy snow showers"],
    95: ["⛈️", "Thunderstorm"],
    96: ["⛈️", "Thunderstorm w/ hail"],
    99: ["⛈️", "Thunderstorm w/ hail"],
  };

  function weatherInfo(code) {
    return WEATHER_CODES[code] || ["🌡️", "Unknown"];
  }

  function getUtcOffsetMinutes(timeZone, date) {
    try {
      const dtf = new Intl.DateTimeFormat("en-US", {
        timeZone,
        timeZoneName: "longOffset",
      });
      const part = dtf.formatToParts(date).find((p) => p.type === "timeZoneName");
      const match = part && part.value.match(/GMT([+-])(\d{1,2})(?::(\d{2}))?/);
      if (!match) return 0;
      const sign = match[1] === "-" ? -1 : 1;
      const hours = parseInt(match[2], 10);
      const minutes = match[3] ? parseInt(match[3], 10) : 0;
      return sign * (hours * 60 + minutes);
    } catch (e) {
      return 0;
    }
  }

  function formatOffsetWithPrefix(prefix, minutes) {
    const sign = minutes < 0 ? "-" : "+";
    const abs = Math.abs(minutes);
    const h = Math.floor(abs / 60);
    const m = abs % 60;
    return `${prefix}${sign}${h}${m ? ":" + String(m).padStart(2, "0") : ""}`;
  }

  function formatOffsetLabel(minutes) {
    return formatOffsetWithPrefix("UTC", minutes);
  }

  function getTimeParts(timeZone, date) {
    const parts = new Intl.DateTimeFormat("en-GB", {
      timeZone,
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    }).formatToParts(date);
    const get = (type) => Number(parts.find((p) => p.type === type)?.value || 0);
    let hour = get("hour");
    if (hour === 24) hour = 0;
    return { hour, minute: get("minute"), second: get("second") };
  }

  function formatGmtDiffLabel(minutes) {
    return formatOffsetWithPrefix("GMT", minutes);
  }

  const CLOCK_TICKS_SVG = Array.from({ length: 12 })
    .map((_, i) => {
      const major = i % 3 === 0;
      return `<line x1="50" y1="5" x2="50" y2="${major ? 12 : 9}" class="clock-tick ${
        major ? "major" : "minor"
      }" transform="rotate(${i * 30} 50 50)" />`;
    })
    .join("");

  function renderAnalogClock(hour, minute, second) {
    const hourAngle = (hour % 12) * 30 + minute * 0.5;
    const minuteAngle = minute * 6 + second * 0.1;
    const secondAngle = second * 6;
    return `
      <svg class="analog-clock" viewBox="0 0 100 100" width="52" height="52">
        <circle cx="50" cy="50" r="46" class="clock-face" />
        ${CLOCK_TICKS_SVG}
        <line x1="50" y1="50" x2="50" y2="27" class="clock-hand hour" transform="rotate(${hourAngle} 50 50)" />
        <line x1="50" y1="50" x2="50" y2="18" class="clock-hand minute" transform="rotate(${minuteAngle} 50 50)" />
        <line x1="50" y1="50" x2="50" y2="14" class="clock-hand second" transform="rotate(${secondAngle} 50 50)" />
        <circle cx="50" cy="50" r="3" class="clock-center" />
      </svg>`;
  }

  function formatTime(timeZone, date) {
    return new Intl.DateTimeFormat("en-GB", {
      timeZone,
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
      hour12: false,
    }).format(date);
  }

  function formatDate(timeZone, date) {
    return new Intl.DateTimeFormat("en-GB", {
      timeZone,
      weekday: "short",
      day: "numeric",
      month: "short",
    }).format(date);
  }

  function regionLabel(place) {
    return [place.admin1, place.country].filter(Boolean).join(", ");
  }

  const state = {
    items: [], // [{place, weather}]
    map: null,
    markersLayer: null,
    detail: {
      placeId: null,
      tab: "today",
      forecast: null, // DetailedForecast from the backend
    },
  };

  const el = {
    cardsGrid: document.getElementById("cards-grid"),
    settingsBtn: document.getElementById("settings-btn"),
    settingsOverlay: document.getElementById("settings-overlay"),
    settingsClose: document.getElementById("settings-close"),
    currentPlaces: document.getElementById("current-places"),
    searchForm: document.getElementById("search-form"),
    searchInput: document.getElementById("search-input"),
    searchStatus: document.getElementById("search-status"),
    searchResults: document.getElementById("search-results"),
    detailOverlay: document.getElementById("detail-overlay"),
    detailClose: document.getElementById("detail-close"),
    detailCity: document.getElementById("detail-city"),
    detailRegion: document.getElementById("detail-region"),
    detailContent: document.getElementById("detail-content"),
    tabButtons: Array.from(document.querySelectorAll(".tab-btn")),
  };

  function sortedItems() {
    const now = new Date();
    return [...state.items].sort(
      (a, b) =>
        getUtcOffsetMinutes(a.place.timezone, now) -
        getUtcOffsetMinutes(b.place.timezone, now)
    );
  }

  function renderCards() {
    const now = new Date();
    const items = sortedItems();

    el.cardsGrid.innerHTML = items
      .map(({ place, weather }) => {
        const offset = getUtcOffsetMinutes(place.timezone, now);
        const { hour, minute, second } = getTimeParts(place.timezone, now);
        const [icon, label] = weatherInfo(weather ? weather.weatherCode : -1);
        const hasWeather = !!weather;
        return `
        <article class="card" data-id="${place.id}">
          <div class="card-header">
            <div class="city-block">
              <div class="city">${escapeHtml(place.name)}</div>
              <div class="region">${escapeHtml(regionLabel(place))}</div>
            </div>
            <div class="offset-block">
              <div class="gmt-diff">${formatGmtDiffLabel(offset)}</div>
              <span class="offset-badge">${formatOffsetLabel(offset)}</span>
            </div>
          </div>
          <div class="time-row">
            <div class="time-block">
              <div class="time">${formatTime(place.timezone, now)}</div>
              <div class="date">${formatDate(place.timezone, now)}</div>
            </div>
            ${renderAnalogClock(hour, minute, second)}
          </div>
          <div class="weather ${hasWeather ? "" : "loading"}">
            <div class="weather-icon">${icon}</div>
            <div class="weather-main">
              <div class="temp">${hasWeather ? Math.round(weather.temperature) + "°C" : "--"}</div>
              <div class="condition">${hasWeather ? label : "Loading…"}</div>
            </div>
            <div class="weather-details ${hasWeather ? "" : "loading"}">
              ${
                hasWeather
                  ? `<span>Feels ${Math.round(weather.apparentTemp)}°C</span>
                     <span>Humidity ${Math.round(weather.humidity)}%</span>
                     <span>Wind ${Math.round(weather.windSpeed)} km/h</span>
                     <span>H:${Math.round(weather.todayHigh)}° L:${Math.round(weather.todayLow)}°</span>`
                  : ""
              }
            </div>
          </div>
        </article>`;
      })
      .join("");
  }

  function escapeHtml(str) {
    const div = document.createElement("div");
    div.textContent = str == null ? "" : String(str);
    return div.innerHTML;
  }

  // A self-contained SVG pin, avoiding Leaflet's default PNG marker images
  // (which don't resolve reliably through the Wails embedded asset server).
  const pinIcon = L.divIcon({
    className: "place-marker",
    html: `<svg width="26" height="34" viewBox="0 0 26 34" xmlns="http://www.w3.org/2000/svg">
      <path d="M13 0C5.8 0 0 5.8 0 13c0 9.75 13 21 13 21s13-11.25 13-21C26 5.8 20.2 0 13 0z" fill="#5b8cff" stroke="#0b0e14" stroke-width="1.5"/>
      <circle cx="13" cy="13" r="5" fill="#0b0e14"/>
    </svg>`,
    iconSize: [26, 34],
    iconAnchor: [13, 34],
    popupAnchor: [0, -30],
  });

  function ensureMap() {
    if (state.map) return;

    state.map = L.map("map", { worldCopyJump: true }).setView([20, 0], 2);
    L.tileLayer("https://{s}.tile.openstreetmap.org/{z}/{x}/{y}.png", {
      attribution: "&copy; OpenStreetMap contributors",
      maxZoom: 18,
    }).addTo(state.map);
    state.markersLayer = L.layerGroup().addTo(state.map);
  }

  function renderMap() {
    ensureMap();
    state.markersLayer.clearLayers();

    const bounds = [];
    const now = new Date();
    for (const { place } of state.items) {
      const marker = L.marker([place.latitude, place.longitude], {
        icon: pinIcon,
      }).addTo(state.markersLayer);
      marker.bindPopup(
        `<div class="map-popup"><div class="popup-city">${escapeHtml(
          place.name
        )}</div><div class="popup-time">${formatTime(
          place.timezone,
          now
        )} &middot; ${formatOffsetLabel(
          getUtcOffsetMinutes(place.timezone, now)
        )}</div></div>`
      );
      bounds.push([place.latitude, place.longitude]);
    }
    if (bounds.length) {
      state.map.fitBounds(bounds, { padding: [40, 40], maxZoom: 5 });
    }
    setTimeout(() => state.map.invalidateSize(), 50);
  }

  function renderSettingsPlaceList() {
    const items = sortedItems();
    const canRemove = items.length > MIN_PLACES;

    el.currentPlaces.innerHTML = items
      .map(
        ({ place }) => `
        <div class="place-row">
          <div>
            <div class="place-name">${escapeHtml(place.name)}</div>
            <div class="place-region">${escapeHtml(regionLabel(place))}</div>
          </div>
          <button class="remove-btn" data-remove-id="${place.id}" ${
          canRemove ? "" : "disabled"
        } title="${canRemove ? "Remove" : `At least ${MIN_PLACES} places required`}">
            Remove
          </button>
        </div>`
      )
      .join("");
  }

  async function loadPlaces() {
    const places = await window.go.main.App.GetPlaces();
    state.items = places.map((place) => ({ place, weather: null }));
    renderCards();
    renderMap();
    renderSettingsPlaceList();
  }

  async function loadWeather() {
    try {
      const combined = await window.go.main.App.GetPlacesWithWeather();
      state.items = combined.map((pw) => ({ place: pw.place, weather: pw.weather }));
      renderCards();
      renderMap();
      renderSettingsPlaceList();
    } catch (e) {
      console.error("Failed to load weather", e);
    }
  }

  function tickClocks() {
    renderCards();
  }

  let searchDebounceTimer = null;
  let searchSeq = 0;

  function openSettings() {
    clearTimeout(searchDebounceTimer);
    el.searchInput.value = "";
    el.searchResults.innerHTML = "";
    el.searchResults._results = [];
    el.searchStatus.textContent = "";
    el.searchStatus.className = "search-status";
    renderSettingsPlaceList();
    el.settingsOverlay.classList.remove("hidden");
    el.searchInput.focus();
  }

  function closeSettings() {
    clearTimeout(searchDebounceTimer);
    el.settingsOverlay.classList.add("hidden");
  }

  async function handleRemove(id) {
    try {
      const updated = await window.go.main.App.RemovePlace(id);
      state.items = mergeWithExistingWeather(updated);
      renderCards();
      renderMap();
      renderSettingsPlaceList();
    } catch (e) {
      console.error("RemovePlace failed:", e);
      showSearchStatus(String(e), "error");
    }
  }

  function mergeWithExistingWeather(places) {
    const byId = new Map(state.items.map((it) => [it.place.id, it.weather]));
    return places.map((place) => ({ place, weather: byId.get(place.id) || null }));
  }

  function showSearchStatus(message, kind) {
    el.searchStatus.textContent = message;
    el.searchStatus.className = "search-status" + (kind ? " " + kind : "");
  }

  function renderSearchResults(results) {
    const atMax = state.items.length >= MAX_PLACES;
    el.searchResults.innerHTML = results
      .map((r, idx) => {
        const region = [r.admin1, r.country].filter(Boolean).join(", ");
        return `
        <div class="place-row">
          <div>
            <div class="place-name">${escapeHtml(r.name)}</div>
            <div class="place-region">${escapeHtml(region)}</div>
          </div>
          <button class="add-btn" data-add-index="${idx}" ${
          atMax ? "disabled" : ""
        } title="${atMax ? `At most ${MAX_PLACES} places allowed` : "Add"}">
            Add
          </button>
        </div>`;
      })
      .join("");
    el.searchResults._results = results;
  }

  async function performSearch(query) {
    const seq = ++searchSeq;
    try {
      const results = await window.go.main.App.SearchCity(query);
      if (seq !== searchSeq) return; // a newer keystroke superseded this search
      showSearchStatus("", "");
      renderSearchResults(results);
    } catch (err) {
      if (seq !== searchSeq) return;
      console.error("SearchCity failed:", err);
      el.searchResults.innerHTML = "";
      el.searchResults._results = [];
      showSearchStatus(String(err), "error");
    }
  }

  function scheduleSearch(rawQuery) {
    const query = rawQuery.trim();
    clearTimeout(searchDebounceTimer);

    if (query.length < 2) {
      searchSeq++; // invalidate any in-flight search
      el.searchResults.innerHTML = "";
      el.searchResults._results = [];
      showSearchStatus("", "");
      return;
    }

    showSearchStatus("Searching…", "info");
    searchDebounceTimer = setTimeout(() => performSearch(query), 350);
  }

  function handleSearchSubmit(e) {
    e.preventDefault();
    clearTimeout(searchDebounceTimer);
    const query = el.searchInput.value.trim();
    if (query.length < 2) return;
    showSearchStatus("Searching…", "info");
    performSearch(query);
  }

  async function handleAdd(index) {
    const results = el.searchResults._results || [];
    const result = results[index];
    if (!result) return;

    try {
      const updated = await window.go.main.App.AddPlace(result);
      state.items = mergeWithExistingWeather(updated);
      renderCards();
      renderMap();
      renderSettingsPlaceList();
      renderSearchResults(results);
      showSearchStatus(`Added ${result.name}.`, "info");
      loadWeather();
    } catch (err) {
      console.error("AddPlace failed:", err);
      showSearchStatus(String(err), "error");
    }
  }

  function formatHourLabel(isoHour) {
    // e.g. "2026-09-11T14:00" -> "14:00"
    const t = isoHour.split("T")[1] || isoHour;
    return t.slice(0, 5);
  }

  function formatDayLabel(dateStr, index) {
    if (index === 0) return "Today";
    const d = new Date(dateStr + "T00:00:00");
    return new Intl.DateTimeFormat("en-GB", { weekday: "short" }).format(d);
  }

  function renderHourly(hourly) {
    if (!hourly || hourly.length === 0) {
      return `<p class="search-status info">No hourly data available.</p>`;
    }
    return `<div class="hourly-row">${hourly
      .map((h) => {
        const [icon] = weatherInfo(h.weatherCode);
        return `
          <div class="hour-card">
            <div class="hour-label">${formatHourLabel(h.time)}</div>
            <div class="hour-icon">${icon}</div>
            <div class="hour-temp">${Math.round(h.temperature)}°C</div>
            <div class="hour-precip">${Math.round(h.precipProb)}%</div>
          </div>`;
      })
      .join("")}</div>`;
  }

  function renderDaily(daily) {
    if (!daily || daily.length === 0) {
      return `<p class="search-status info">No forecast data available.</p>`;
    }
    return `<div class="daily-list">${daily
      .map((d, idx) => {
        const [icon, label] = weatherInfo(d.weatherCode);
        return `
          <div class="day-row">
            <div class="day-name">${formatDayLabel(d.date, idx)}</div>
            <div class="day-icon">${icon}</div>
            <div class="day-condition">${label}</div>
            <div class="day-precip">💧${Math.round(d.precipProb)}%</div>
            <div class="day-temps">${Math.round(d.high)}° <span class="low">${Math.round(d.low)}°</span></div>
          </div>`;
      })
      .join("")}</div>`;
  }

  function renderDetailContent() {
    const { tab, forecast } = state.detail;
    if (!forecast) return;
    el.detailContent.innerHTML =
      tab === "today" ? renderHourly(forecast.hourly) : renderDaily(forecast.daily);
  }

  function setActiveTab(tab) {
    state.detail.tab = tab;
    el.tabButtons.forEach((btn) => btn.classList.toggle("active", btn.dataset.tab === tab));
    renderDetailContent();
  }

  async function openDetail(placeId) {
    const item = state.items.find((it) => it.place.id === placeId);
    if (!item) return;

    state.detail.placeId = placeId;
    state.detail.forecast = null;
    setActiveTab("today");

    el.detailCity.textContent = item.place.name;
    el.detailRegion.textContent = regionLabel(item.place);
    el.detailContent.innerHTML = `<p class="search-status info">Loading&hellip;</p>`;
    el.detailOverlay.classList.remove("hidden");

    try {
      const forecast = await window.go.main.App.GetDetailedForecast(
        item.place.latitude,
        item.place.longitude
      );
      if (state.detail.placeId !== placeId) return; // closed/switched before this resolved
      state.detail.forecast = forecast;
      renderDetailContent();
    } catch (err) {
      console.error("GetDetailedForecast failed:", err);
      if (state.detail.placeId !== placeId) return;
      el.detailContent.innerHTML = `<p class="search-status error">${escapeHtml(String(err))}</p>`;
    }
  }

  function closeDetail() {
    state.detail.placeId = null;
    el.detailOverlay.classList.add("hidden");
  }

  function wireEvents() {
    el.settingsBtn.addEventListener("click", openSettings);
    el.settingsClose.addEventListener("click", closeSettings);
    el.settingsOverlay.addEventListener("click", (e) => {
      if (e.target === el.settingsOverlay) closeSettings();
    });
    document.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        closeSettings();
        closeDetail();
      }
    });

    el.cardsGrid.addEventListener("click", (e) => {
      const weatherEl = e.target.closest(".weather");
      if (!weatherEl) return;
      const card = e.target.closest(".card[data-id]");
      if (card) openDetail(card.dataset.id);
    });

    el.detailClose.addEventListener("click", closeDetail);
    el.detailOverlay.addEventListener("click", (e) => {
      if (e.target === el.detailOverlay) closeDetail();
    });
    el.tabButtons.forEach((btn) => {
      btn.addEventListener("click", () => setActiveTab(btn.dataset.tab));
    });

    el.searchForm.addEventListener("submit", handleSearchSubmit);
    el.searchInput.addEventListener("input", (e) => scheduleSearch(e.target.value));

    el.currentPlaces.addEventListener("click", (e) => {
      const btn = e.target.closest("[data-remove-id]");
      if (btn && !btn.disabled) handleRemove(btn.dataset.removeId);
    });

    el.searchResults.addEventListener("click", (e) => {
      const btn = e.target.closest("[data-add-index]");
      if (btn && !btn.disabled) handleAdd(Number(btn.dataset.addIndex));
    });

    window.addEventListener("resize", () => {
      if (state.map) state.map.invalidateSize();
    });
  }

  async function main() {
    wireEvents();
    await loadPlaces();
    await loadWeather();
    setInterval(tickClocks, 1000);
    setInterval(loadWeather, 10 * 60 * 1000);
  }

  window.addEventListener("DOMContentLoaded", main);
})();
