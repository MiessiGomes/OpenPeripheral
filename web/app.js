const $ = (id) => document.getElementById(id);
const FX = [
  { kind: "blink", title: "Piscar", hint: "Liga e apaga os dois anéis" },
  { kind: "pulse", title: "Pulso", hint: "Sobe e desce o brilho da cor" },
  { kind: "cycle", title: "Ciclo", hint: "Varre o espectro nos dois anéis" },
  { kind: "wave", title: "Onda", hint: "Topo e base em fases opostas" },
  { kind: "lightning", title: "Relâmpago", hint: "Estalos alternados nos anéis" },
];
let applying = false;
let view = "dash";
let lastState = null;
let selectedId = null;
let pickKind = null;

const PAGE_OPTS = {
  light: {
    title: "Iluminação",
    hint: "Cor, brilho e efeitos do LED.",
  },
  audio: {
    title: "Áudio",
    hint: "Polar, ganho, high-pass e teste no PC.",
  },
  mouse: {
    title: "DPI e sensor",
    hint: "Taxa, LOD, bateria e botões deste periférico.",
  },
};

const VIEW_TITLES = {
  dash: ["Dashboard", "Escolha um periférico para abrir as opções."],
  device: ["Periférico", "Iluminação e áudio deste dispositivo."],
  "pick-light": ["Iluminação", "Escolha o periférico para ajustar o LED."],
  "pick-audio": ["Áudio", "Escolha o periférico para captura e teste."],
  light: ["Iluminação", "LED do periférico escolhido."],
  audio: ["Áudio", "Ouça o DuoCast no PC e teste polar, high-pass e ganho."],
  mouse: ["DPI e sensor", "Taxa, LOD, bateria e botões do periférico escolhido."],
};

function paintMic(id, top, bot, effect, speed) {
  if (window.OpenPeripheralMic) {
    OpenPeripheralMic.mount($(id));
    if (effect === "cycle" || effect === "wave") OpenPeripheralMic.setRainbow(true, speed);
    else OpenPeripheralMic.setColors(top, bot);
  }
}

async function api(path, body) {
  const res = await fetch(path, {
    method: body ? "POST" : "GET",
    headers: body ? { "Content-Type": "application/json" } : {},
    body: body ? JSON.stringify(body) : undefined,
  });
  const data = await res.json();
  if (!res.ok) throw new Error(data.error || res.statusText);
  return data;
}

function pagesOf(p) {
  return p.pages && p.pages.length ? p.pages : ["light", "audio"];
}

function findPeripheral(id) {
  return (lastState && lastState.peripherals ? lastState.peripherals : []).find((p) => p.id === id);
}

function renderPeripherals(list, host, onPick) {
  host.innerHTML = "";
  if (!list.length) {
    host.innerHTML = "<p class='hint'>Nenhum periférico visível.</p>";
    return;
  }
  for (const p of list) {
    const el = document.createElement("button");
    el.type = "button";
    el.className = "peripheral";
    const bits = (p.details || []).map((d) => `<div><code>${d}</code></div>`).join("");
    el.innerHTML = `<header><span class="kind">${p.brand} · ${p.kind}</span><span class="pill ${p.connected ? "ok" : ""}">${p.connected ? "ON" : "OFF"}</span></header><b>${p.name}</b><div class="sum">${p.summary}</div>${bits}`;
    el.onclick = () => onPick(p);
    host.appendChild(el);
  }
}

function fillDeviceHub() {
  const p = findPeripheral(selectedId);
  if (!p) {
    $("device-kicker").textContent = "Periférico";
    $("device-sum").textContent = "Dispositivo não encontrado.";
    $("device-options").innerHTML = "";
    return;
  }
  $("page-title").textContent = p.name;
  $("device-kicker").textContent = `${p.brand} · ${p.kind}`;
  $("device-sum").textContent = p.connected
    ? "Escolha o que deseja ajustar."
    : "Desconectado — as páginas ainda abrem, mas o HID pode falhar.";
  const box = $("device-options");
  box.innerHTML = "";
  for (const page of pagesOf(p)) {
    const meta = PAGE_OPTS[page];
    if (!meta) continue;
    const b = document.createElement("button");
    b.type = "button";
    b.className = "opt-card";
    b.innerHTML = `<strong>${meta.title}</strong><small>${meta.hint}</small>`;
    b.onclick = () => openPage(page, p.id);
    box.appendChild(b);
  }
}

function fillPicker() {
  const need = pickKind === "audio" ? "audio" : "light";
  $("pick-kicker").textContent = need === "light" ? "Iluminação · periférico" : "Áudio · periférico";
  $("pick-hint").textContent =
    need === "light"
      ? "Clique no periférico para abrir a iluminação."
      : "Clique no periférico para captura e teste.";
  const list = ((lastState && lastState.peripherals) || []).filter((p) => pagesOf(p).includes(need));
  renderPeripherals(list, $("pick-list"), (p) => openPage(need, p.id));
}

function openDevice(id) {
  selectedId = id;
  show("device");
}

function openPage(page, id) {
  selectedId = id;
  show(page);
}

function render(s) {
  lastState = s;
  $("led-pill").textContent = s.lighting_ok ? "LED ON" : "LED OFF";
  $("led-pill").classList.toggle("ok", s.lighting_ok);
  const mouseOn = !!(s.aj179 && s.aj179.connected);
  $("mouse-pill").textContent = mouseOn ? "MOUSE ON" : "MOUSE OFF";
  $("mouse-pill").classList.toggle("ok", mouseOn);
  setAudioBadge(s.muted);
  $("notice").textContent = s.notice || "";
  const upd = $("update");
  if (s.update && s.update.available && s.update.latest) {
    upd.classList.remove("hidden");
    const href = s.update.url || "https://github.com/MiessiGomes/OpenPeripheral/releases";
    upd.innerHTML = `Há uma versão nova: <b>${s.update.latest}</b> (instalada: ${s.update.current || s.version || "—"}). <a href="${href}" target="_blank" rel="noopener">Abrir no GitHub</a>`;
  } else {
    upd.classList.add("hidden");
    upd.innerHTML = "";
  }
  const perm = $("perm");
  if (s.permission_hint) {
    perm.classList.remove("hidden");
    perm.textContent = s.udev_installed
      ? "O DuoCast aparece no USB, mas o LED HID não abre. Reconecte o cabo para as regras udev do pacote valerem."
      : "O DuoCast aparece no USB, mas o LED HID não abre. Instale o .deb (udev no postinst) ou, em desenvolvimento, copie udev/99-hyperx-duocast.rules uma vez.";
  } else {
    perm.classList.add("hidden");
  }

  renderPeripherals(s.peripherals || [], $("peripherals"), (p) => openDevice(p.id));
  if (view === "device") fillDeviceHub();
  if (view === "pick-light" || view === "pick-audio") fillPicker();
  paintAj179(s.aj179 || {});

  paintMic("light-mic", s.led.top, s.led.bottom, s.led.effect, s.led.speed);
  setMeter("meter", "hold", s.monitor);
  $("listen").classList.toggle("on", s.monitor.listening);
  $("listen").textContent = s.monitor.listening ? "Parar de ouvir" : "Ouvir no PC";
  $("record").classList.toggle("on", s.monitor.recording);
  $("play").disabled = !s.monitor.has_take && !s.monitor.playing;
  const bits = [];
  if (s.monitor.input) bits.push("in " + s.monitor.input);
  if (s.monitor.output) bits.push("out " + s.monitor.output);
  if (s.monitor.recording) bits.push("gravando 4s");
  if (s.monitor.playing) bits.push("reproduzindo");
  $("mon-io").textContent = bits.join(" · ");
  $("mon-err").textContent = s.monitor.error || "";
  if (!applying) $("mon-vol").value = s.monitor.volume;

  if (!applying) {
    $("top").value = s.led.top;
    $("bottom").value = s.led.bottom;
    $("link").checked = s.led.link;
    $("bri").value = s.led.brightness;
    $("bottom").disabled = s.led.link;
    $("mic").checked = s.mixer.mic_capture;
    $("ext").checked = s.mixer.extension;
    $("pcm").value = s.mixer.pcm_percent;
    $("hmute").checked = !!s.hid.mute;
    $("st").checked = !!s.hid.sidetone_mute;
  }
  $("mixer-err").textContent = s.mixer_error || "";
  $("hid-err").textContent = s.hid.error || "";
  if (s.hid.ping) $("ping-out").textContent = s.hid.ping;

  document.querySelectorAll("#fx .fx-btn").forEach((b) => {
    b.classList.toggle("on", s.led.effect === b.dataset.kind);
  });
  const fxOn = !!s.led.effect;
  $("fx-speed-wrap").classList.toggle("is-off", !fxOn);
  $("fx-speed").disabled = !fxOn;
  if (!applying && s.led.speed != null && document.activeElement !== $("fx-speed")) {
    $("fx-speed").value = s.led.speed;
  }
  syncFxSpeedLabel();
  $("polar-c").classList.toggle("accent", s.hid.polar === "cardioid");
  $("polar-o").classList.toggle("accent", s.hid.polar === "omni");
}

async function refresh() {
  try {
    render(await api("/api/state"));
  } catch (e) {
    $("notice").textContent = e.message;
  }
}

async function sendLed(extra) {
  applying = true;
  const body = {
    mode: extra.mode || "solid",
    top: $("top").value,
    bottom: $("link").checked ? $("top").value : $("bottom").value,
    brightness: Number($("bri").value),
    link: $("link").checked,
    ...extra,
  };
  try {
    render(await api("/api/led", body));
  } catch (e) {
    $("notice").textContent = e.message;
  } finally {
    applying = false;
  }
}

async function sendAj179(body) {
  applying = true;
  try {
    render(await api("/api/aj179", body));
  } catch (e) {
    $("notice").textContent = e.message;
  } finally {
    applying = false;
  }
}

function paintAj179(m) {
  if (!m) return;
  const id = m.device_id || "AJ179";
  const bat = m.battery == null ? "—" : m.battery + "%";
  const link = m.link || "";
  const awake = m.awake ? "acordado" : "a dormir";
  $("aj-sum").textContent = m.connected
    ? `${id} · ${link} · ${awake}${m.rx_fw ? " · fw " + m.rx_fw : ""}`
    : "AJ179 desconectado.";
  $("aj-bat").textContent = m.charging ? `⚡ ${bat}` : `bateria ${bat}`;
  $("aj-err").textContent = m.error || "";
  if (!applying) {
    if (m.rate_hz) $("aj-rate").value = String(m.rate_hz);
    if (m.lod) $("aj-lod").value = String(m.lod);
    $("aj-ms").checked = !!m.motion_sync;
    $("aj-as").checked = !!m.angle_snap;
    if (m.light_color && m.light_color !== "#000000") $("aj-color").value = m.light_color;
    $("aj-bri").value = m.light_brightness > 0 ? m.light_brightness : 4;
    $("aj-spd").value = m.light_speed;
  }
  const lightNames = { 0: "desligado", 2: "respiração", 3: "constante", 4: "néon" };
  const lightEl = $("aj-light-sum");
  if (lightEl) {
    const name = lightNames[m.light_mode] || (m.light_mode == null ? "—" : "modo " + m.light_mode);
    lightEl.textContent = m.connected
      ? `Atual: ${name} · ${m.light_color || "#000000"} · brilho ${m.light_brightness ?? "—"}`
      : "";
  }
  paintDpiRows(m);
  paintButtons(m);
}

const AJ_BTN_OPTS = [
  ["left", "Esquerdo"],
  ["right", "Direito"],
  ["middle", "Meio"],
  ["back", "Traseiro"],
  ["forward", "Dianteiro"],
  ["scroll-up", "Scroll ↑"],
  ["scroll-down", "Scroll ↓"],
  ["dpi-loop", "DPI ciclo"],
  ["dpi-up", "DPI +"],
  ["dpi-down", "DPI −"],
  ["play-pause", "Play/Pause"],
  ["next", "Seguinte"],
  ["previous", "Anterior"],
  ["stop", "Stop"],
  ["volume-up", "Volume +"],
  ["volume-down", "Volume −"],
  ["mute", "Mudo"],
  ["disabled", "Desligado"],
];

function paintDpiRows(m) {
  const box = $("aj-dpi-stages");
  const stages = m.dpi || [];
  if (box.childElementCount !== stages.length) {
    box.innerHTML = "";
    stages.forEach((d, i) => {
      const row = document.createElement("div");
      row.className = "dpi-row";
      row.innerHTML = `<span>${i + 1}</span><input type="number" min="50" max="26000" step="50" id="aj-dpi-val-${i}" /><input type="color" id="aj-dpi-col-${i}" /><button type="button" class="pick" data-i="${i}">Usar</button>`;
      box.appendChild(row);
      const val = $(`aj-dpi-val-${i}`);
      const col = $(`aj-dpi-col-${i}`);
      val.onchange = () => commitDpi();
      col.onchange = () => commitDpi();
      row.querySelector(".pick").onclick = () => sendAj179({ dpi_index: i });
    });
  }
  stages.forEach((d, i) => {
    const val = $(`aj-dpi-val-${i}`);
    const col = $(`aj-dpi-col-${i}`);
    const btn = box.querySelector(`.pick[data-i="${i}"]`);
    if (btn) btn.classList.toggle("accent", m.dpi_index === i);
    if (!applying && val && document.activeElement !== val) val.value = d.value;
    if (!applying && col && document.activeElement !== col) col.value = d.color || "#ff0000";
  });
}

function commitDpi() {
  const box = $("aj-dpi-stages");
  const n = box.querySelectorAll(".dpi-row").length;
  const dpi_values = [];
  const dpi_colors = [];
  for (let i = 0; i < n; i++) {
    dpi_values.push(Number($(`aj-dpi-val-${i}`).value));
    dpi_colors.push($(`aj-dpi-col-${i}`).value);
  }
  sendAj179({ dpi_values, dpi_colors });
}

function paintButtons(m) {
  const host = $("aj-buttons");
  const list = m.buttons || [];
  if (host.childElementCount !== list.length) {
    host.innerHTML = "";
    list.forEach((b) => {
      const lab = document.createElement("label");
      lab.className = "field";
      const sel = document.createElement("select");
      sel.id = `aj-btn-${b.index}`;
      sel.disabled = !!b.locked;
      for (const [v, t] of AJ_BTN_OPTS) {
        const o = document.createElement("option");
        o.value = v;
        o.textContent = t;
        sel.appendChild(o);
      }
      sel.onchange = () => sendAj179({ button_index: b.index, button_action: sel.value });
      lab.append(`${b.index + 1}. ${b.name}`, sel);
      host.appendChild(lab);
    });
  }
  list.forEach((b) => {
    const sel = $(`aj-btn-${b.index}`);
    if (!sel) return;
    const known = AJ_BTN_OPTS.some((o) => o[0] === b.action);
    if (!known && b.action) {
      let extra = [...sel.options].find((o) => o.value === b.action);
      if (!extra) {
        extra = document.createElement("option");
        extra.value = b.action;
        extra.textContent = b.action;
        sel.appendChild(extra);
      }
    }
    if (!applying && document.activeElement !== sel) sel.value = b.action;
  });
}

async function sendAudio(body) {
  applying = true;
  try {
    render(await api("/api/audio", body));
  } catch (e) {
    $("notice").textContent = e.message;
  } finally {
    applying = false;
  }
}

function navFor(name) {
  if (name === "pick-light" || name === "light") return "light";
  if (name === "pick-audio" || name === "audio") return "audio";
  return "dash";
}

function show(name) {
  view = name;
  if (name === "pick-light") pickKind = "light";
  if (name === "pick-audio") pickKind = "audio";
  document.querySelectorAll("nav button").forEach((b) => {
    b.classList.toggle("active", b.dataset.nav === navFor(name));
  });
  $("view-dash").classList.toggle("hidden", name !== "dash");
  $("view-device").classList.toggle("hidden", name !== "device");
  $("view-pick").classList.toggle(
    "hidden",
    name !== "pick-light" && name !== "pick-audio"
  );
  $("view-light").classList.toggle("hidden", name !== "light");
  $("view-audio").classList.toggle("hidden", name !== "audio");
  $("view-mouse").classList.toggle("hidden", name !== "mouse");
  const ajLight = selectedId === "aj179";
  $("duo-light-card").classList.toggle("hidden", name === "light" && ajLight);
  $("duo-fx-card").classList.toggle("hidden", name === "light" && ajLight);
  $("aj-light-card").classList.toggle("hidden", !(name === "light" && ajLight));
  const titles = VIEW_TITLES[name] || VIEW_TITLES.dash;
  $("page-title").textContent = titles[0];
  $("page-sub").textContent = titles[1];
  if (name === "device") fillDeviceHub();
  if (name === "pick-light" || name === "pick-audio") fillPicker();
}

function initFx() {
  const root = $("fx");
  for (const fx of FX) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "fx-btn";
    b.dataset.kind = fx.kind;
    b.innerHTML = `<strong>${fx.title}</strong><small>${fx.hint}</small>`;
    b.onclick = () =>
      sendLed({ mode: "effect", kind: fx.kind, speed: Number($("fx-speed").value) });
    root.appendChild(b);
  }
}

function syncFxSpeedLabel() {
  $("fx-speed-val").textContent = (Number($("fx-speed").value) / 100).toFixed(1) + "×";
}

document.querySelectorAll("nav button").forEach((b) => {
  b.onclick = () => show(b.dataset.view);
});
$("device-back").onclick = () => show("dash");
$("pick-back").onclick = () => show("dash");
$("apply").onclick = () => sendLed({ mode: "solid" });
$("save").onclick = () => sendLed({ mode: "solid", save: true });
$("off").onclick = () => sendLed({ mode: "off" });
$("reset").onclick = () => sendLed({ mode: "reset" });
let ledTimer;
function debounceLed() {
  clearTimeout(ledTimer);
  ledTimer = setTimeout(() => sendLed({ mode: "solid" }), 80);
}
$("top").oninput = () => {
  if ($("link").checked) $("bottom").value = $("top").value;
  debounceLed();
};
$("bottom").oninput = debounceLed;
$("bri").oninput = debounceLed;
$("link").onchange = () => {
  $("bottom").disabled = $("link").checked;
  sendLed({ mode: "solid" });
};
let fxSpeedTimer;
$("fx-speed").oninput = () => {
  syncFxSpeedLabel();
  const on = document.querySelector("#fx .fx-btn.on");
  if (!on) return;
  clearTimeout(fxSpeedTimer);
  fxSpeedTimer = setTimeout(
    () => sendLed({ mode: "effect", kind: on.dataset.kind, speed: Number($("fx-speed").value) }),
    80
  );
};
$("mic").onchange = () => sendAudio({ mic_capture: $("mic").checked });
$("ext").onchange = () => sendAudio({ extension: $("ext").checked });
$("pcm").onchange = () => sendAudio({ pcm_percent: Number($("pcm").value) });
$("polar-c").onclick = () => sendAudio({ polar: "cardioid" });
$("polar-o").onclick = () => sendAudio({ polar: "omni" });
$("hmute").onchange = () => sendAudio({ hid_mute: $("hmute").checked });
let hpTimer;
$("hp").oninput = () => {
  clearTimeout(hpTimer);
  hpTimer = setTimeout(() => sendAudio({ highpass: Number($("hp").value) }), 80);
};
$("st").onchange = () => sendAudio({ sidetone_mute: $("st").checked });
let gainTimer;
$("gain").oninput = () => {
  clearTimeout(gainTimer);
  gainTimer = setTimeout(() => sendAudio({ gain: Number($("gain").value) }), 80);
};
$("ping").onclick = () => sendAudio({ ping: true });
$("aj-rate").onchange = () => sendAj179({ rate: Number($("aj-rate").value) });
$("aj-lod").onchange = () => sendAj179({ lod: Number($("aj-lod").value) });
$("aj-ms").onchange = () => sendAj179({ motion_sync: $("aj-ms").checked });
$("aj-as").onchange = () => sendAj179({ angle_snap: $("aj-as").checked });
function ajLightBody(mode) {
  return {
    light_mode: mode,
    color: $("aj-color").value,
    brightness: Math.max(1, Number($("aj-bri").value) || 4),
    speed: Number($("aj-spd").value),
  };
}
$("aj-const").onclick = () => sendAj179(ajLightBody("constant"));
$("aj-breath").onclick = () => sendAj179(ajLightBody("breathing"));
$("aj-led-off").onclick = () => sendAj179({ light_mode: "off", brightness: 0, speed: 0 });
$("aj-btn-reset").onclick = () => sendAj179({ buttons_reset: true });

async function sendMonitor(body) {
  applying = true;
  try {
    render(await api("/api/monitor", body));
  } catch (e) {
    $("notice").textContent = e.message;
  } finally {
    applying = false;
  }
}

$("listen").onclick = () => {
  const on = !$("listen").classList.contains("on");
  sendMonitor({ listen: on, volume: Number($("mon-vol").value) });
};
$("record").onclick = () => sendMonitor({ action: "record", volume: Number($("mon-vol").value) });
$("play").onclick = () => sendMonitor({ action: "play" });
let volTimer;
$("mon-vol").oninput = () => {
  clearTimeout(volTimer);
  volTimer = setTimeout(() => sendMonitor({ volume: Number($("mon-vol").value) }), 80);
};

function setAudioBadge(muted) {
  $("audio-pill").textContent = muted ? "ÁUDIO OFF" : "ÁUDIO ON";
  $("audio-pill").classList.toggle("ok", !muted);
}

function setMeter(fillId, holdId, mon) {
  const pct = Math.min(100, (mon.peak || 0) * 140);
  const hold = Math.min(100, (mon.hold || 0) * 140);
  $(fillId).style.width = pct + "%";
  $(holdId).style.left = hold + "%";
}

initFx();
show("dash");
refresh();
setInterval(refresh, 1500);
setInterval(async () => {
  try {
    const m = await api("/api/meter");
    setMeter("meter", "hold", m);
    if (typeof m.muted === "boolean") setAudioBadge(m.muted);
  } catch (_) {}
}, 80);
