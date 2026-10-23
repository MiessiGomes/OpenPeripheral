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
    hint: "Cor, brilho e efeitos do anel RGB.",
  },
  audio: {
    title: "Áudio",
    hint: "Polar, ganho, high-pass e teste no PC.",
  },
};

const VIEW_TITLES = {
  dash: ["Dashboard", "Escolha um periférico para abrir as opções."],
  device: ["Periférico", "Iluminação e áudio deste dispositivo."],
  "pick-light": ["Iluminação", "Escolha o periférico para ajustar o LED."],
  "pick-audio": ["Áudio", "Escolha o periférico para captura e teste."],
  light: ["Iluminação", "Dois anéis no controlador 03F0:098C, interface 0."],
  audio: ["Áudio", "Ouça o DuoCast no PC e teste polar, high-pass e ganho."],
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
      ? "Clique no microfone (ou outro dispositivo com LED) para abrir a iluminação."
      : "Clique no microfone (ou outro dispositivo com captura) para abrir o áudio.";
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
  if (name === "device") return "dash";
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
  $("view-pick").classList.toggle("hidden", name !== "pick-light" && name !== "pick-audio");
  $("view-light").classList.toggle("hidden", name !== "light");
  $("view-audio").classList.toggle("hidden", name !== "audio");
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
