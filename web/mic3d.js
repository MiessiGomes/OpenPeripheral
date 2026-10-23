/* Modelo 3D DuoCast (Three.js r128), a partir de gemini/3d_mic.html */
(function (global) {
  if (!global.THREE) {
    console.warn("Three.js não carregou");
    return;
  }

  const viewers = [];
  let rainbow = false;
  let rainbowSpeed = 1;
  let clock = new THREE.Clock();

  function grilleTexture() {
    const canvas = document.createElement("canvas");
    canvas.width = 512;
    canvas.height = 512;
    const ctx = canvas.getContext("2d");
    ctx.fillStyle = "#18191c";
    ctx.fillRect(0, 0, 512, 512);
    ctx.fillStyle = "#050506";
    for (let r = 0; r < 32; r++) {
      for (let c = 0; c < 32; c++) {
        const x = (c + (r % 2 === 0 ? 0 : 0.5)) * (512 / 32);
        const y = r * (512 / 32);
        ctx.beginPath();
        ctx.arc(x, y, 5, 0, Math.PI * 2);
        ctx.fill();
      }
    }
    const t = new THREE.CanvasTexture(canvas);
    t.wrapS = t.wrapT = THREE.RepeatWrapping;
    t.repeat.set(2, 2);
    return t;
  }

  function createModel(rgbTopMat, rgbBotMat, rgbLight) {
    const micGroup = new THREE.Group();
    const bodyMaterial = new THREE.MeshStandardMaterial({
      color: 0x1a1c20,
      roughness: 0.35,
      metalness: 0.8,
    });
    const mountMaterial = new THREE.MeshStandardMaterial({
      color: 0x121316,
      roughness: 0.5,
      metalness: 0.5,
    });

    const innerTex = (() => {
      const c = document.createElement("canvas");
      c.width = c.height = 256;
      const x = c.getContext("2d");
      x.fillStyle = "#08080a";
      x.fillRect(0, 0, 256, 256);
      x.fillStyle = "#1a1a20";
      for (let i = 0; i < 256; i += 4) {
        x.fillRect(i, 0, 2, 256);
        x.fillRect(0, i, 256, 2);
      }
      const t = new THREE.CanvasTexture(c);
      t.wrapS = t.wrapT = THREE.RepeatWrapping;
      t.repeat.set(8, 8);
      return t;
    })();

    const grilleMat = new THREE.MeshStandardMaterial({
      color: 0x222428,
      roughness: 0.4,
      metalness: 0.6,
      bumpMap: grilleTexture(),
      bumpScale: 0.03,
    });
    const grille = new THREE.Mesh(new THREE.CylinderGeometry(0.8, 0.8, 2.2, 64), grilleMat);
    grille.position.y = 1.3;
    grille.castShadow = true;
    micGroup.add(grille);

    const inner = new THREE.Mesh(
      new THREE.CylinderGeometry(0.76, 0.76, 2.18, 32),
      new THREE.MeshStandardMaterial({ color: 0x050505, roughness: 0.9, map: innerTex })
    );
    inner.position.y = 1.3;
    micGroup.add(inner);

    const topCap = new THREE.Mesh(new THREE.CylinderGeometry(0.8, 0.8, 0.1, 64), bodyMaterial);
    topCap.position.y = 2.45;
    micGroup.add(topCap);

    const rgbTop = new THREE.Mesh(new THREE.CylinderGeometry(0.81, 0.81, 0.07, 64), rgbTopMat);
    rgbTop.position.y = 0.22;
    micGroup.add(rgbTop);
    const rgbBot = new THREE.Mesh(new THREE.CylinderGeometry(0.81, 0.81, 0.07, 64), rgbBotMat);
    rgbBot.position.y = 0.14;
    micGroup.add(rgbBot);
    rgbLight.position.set(0, 0.18, 0);
    micGroup.add(rgbLight);

    const lower = new THREE.Mesh(new THREE.CylinderGeometry(0.8, 0.8, 1.4, 64), bodyMaterial);
    lower.position.y = -0.58;
    lower.castShadow = true;
    micGroup.add(lower);
    const bottomCap = new THREE.Mesh(new THREE.CylinderGeometry(0.8, 0.75, 0.2, 64), bodyMaterial);
    bottomCap.position.y = -1.33;
    micGroup.add(bottomCap);

    const knob = new THREE.Group();
    knob.position.set(0, 0.18, 0.81);
    const knobBase = new THREE.Mesh(new THREE.CylinderGeometry(0.2, 0.2, 0.08, 32).rotateX(Math.PI / 2), bodyMaterial);
    const knobDial = new THREE.Mesh(
      new THREE.CylinderGeometry(0.14, 0.14, 0.12, 32).rotateX(Math.PI / 2),
      new THREE.MeshStandardMaterial({ color: 0x0d0e10, roughness: 0.2 })
    );
    knobDial.position.z = 0.04;
    knob.add(knobBase, knobDial);
    micGroup.add(knob);

    const iconMat = new THREE.MeshBasicMaterial({ color: 0x88909a });
    const minus = new THREE.Mesh(new THREE.BoxGeometry(0.06, 0.015, 0.01), iconMat);
    minus.position.set(-0.28, 0.25, 0.79);
    const plus = new THREE.Group();
    plus.add(new THREE.Mesh(new THREE.BoxGeometry(0.06, 0.015, 0.01), iconMat));
    plus.add(new THREE.Mesh(new THREE.BoxGeometry(0.015, 0.06, 0.01), iconMat));
    plus.position.set(0.28, 0.25, 0.79);
    micGroup.add(minus, plus);

    const jack = new THREE.Mesh(
      new THREE.CylinderGeometry(0.05, 0.05, 0.1, 16).rotateX(Math.PI / 2),
      new THREE.MeshBasicMaterial({ color: 0x000000 })
    );
    jack.position.set(0, -0.6, 0.8);
    const usbc = new THREE.Mesh(new THREE.BoxGeometry(0.12, 0.05, 0.08), new THREE.MeshBasicMaterial({ color: 0x050505 }));
    usbc.position.set(0, -0.9, 0.79);
    micGroup.add(jack, usbc);

    const mountGroup = new THREE.Group();
    const outer = new THREE.Mesh(new THREE.TorusGeometry(1.15, 0.08, 16, 64).rotateX(Math.PI / 2), mountMaterial);
    outer.position.y = -0.15;
    mountGroup.add(outer);
    for (let i = 0; i < 4; i++) {
      const ang = (i * Math.PI) / 2 + Math.PI / 4;
      const el = new THREE.Mesh(
        new THREE.CylinderGeometry(0.02, 0.02, 0.35, 8),
        new THREE.MeshBasicMaterial({ color: 0x080808 })
      );
      el.position.set(Math.cos(ang) * 0.95, -0.15, Math.sin(ang) * 0.95);
      el.rotation.y = -ang;
      el.rotation.z = Math.PI / 2;
      mountGroup.add(el);
    }
    const arm = new THREE.Mesh(new THREE.CylinderGeometry(0.08, 0.08, 1.8, 32), mountMaterial);
    arm.position.set(0, -1.0, -1.15);
    mountGroup.add(arm);
    const joint = new THREE.Mesh(
      new THREE.CylinderGeometry(0.12, 0.12, 0.3, 32).rotateZ(Math.PI / 2),
      mountMaterial
    );
    joint.position.set(0, -0.15, -1.15);
    mountGroup.add(joint);
    const base = new THREE.Mesh(new THREE.CylinderGeometry(1.2, 1.25, 0.1, 64), mountMaterial);
    base.position.set(0, -1.9, -0.8);
    mountGroup.add(base);

    micGroup.position.y = 0.3;
    mountGroup.position.y = 0.3;
    return { micGroup, mountGroup };
  }

  function mount(el) {
    if (!el || el.dataset.ready) return;
    el.dataset.ready = "1";
    const w = el.clientWidth || 280;
    const h = el.clientHeight || 320;

    const scene = new THREE.Scene();
    scene.background = new THREE.Color(0x0c0e12);
    scene.fog = new THREE.FogExp2(0x0c0e12, 0.045);

    const camera = new THREE.PerspectiveCamera(40, w / h, 0.1, 100);
    camera.position.set(0, 1.2, 7.2);

    const renderer = new THREE.WebGLRenderer({ antialias: true, alpha: true });
    renderer.setSize(w, h);
    renderer.setPixelRatio(Math.min(window.devicePixelRatio || 1, 2));
    renderer.shadowMap.enabled = true;
    renderer.toneMapping = THREE.ACESFilmicToneMapping;
    el.appendChild(renderer.domElement);

    const controls = new THREE.OrbitControls(camera, renderer.domElement);
    controls.enableDamping = true;
    controls.target.set(0, 0.2, 0);
    controls.minDistance = 4;
    controls.maxDistance = 12;

    scene.add(new THREE.AmbientLight(0xffffff, 0.7));
    const key = new THREE.DirectionalLight(0xffffff, 1.6);
    key.position.set(5, 8, 5);
    scene.add(key);
    scene.add(new THREE.DirectionalLight(0x80a0ff, 0.5).translateX(-5));

    const rgbTopMat = new THREE.MeshBasicMaterial({ color: 0xf20000 });
    const rgbBotMat = new THREE.MeshBasicMaterial({ color: 0xf20000 });
    const rgbLight = new THREE.PointLight(0xf20000, 1.4, 3);
    const parts = createModel(rgbTopMat, rgbBotMat, rgbLight);
    scene.add(parts.micGroup);
    scene.add(parts.mountGroup);

    const viewer = {
      el,
      scene,
      camera,
      renderer,
      controls,
      rgbTopMat,
      rgbBotMat,
      rgbLight,
      autoRotate: true,
    };
    viewers.push(viewer);

    const ro = new ResizeObserver(() => {
      const nw = el.clientWidth || 280;
      const nh = el.clientHeight || 320;
      camera.aspect = nw / nh;
      camera.updateProjectionMatrix();
      renderer.setSize(nw, nh);
    });
    ro.observe(el);

    function tick() {
      requestAnimationFrame(tick);
      if (viewer.autoRotate) parts.micGroup.rotation.y += 0.006;
      if (rainbow) {
        const hue = (clock.getElapsedTime() * 0.25 * rainbowSpeed) % 1;
        const c = new THREE.Color().setHSL(hue, 1, 0.5);
        rgbTopMat.color.copy(c);
        rgbBotMat.color.copy(c);
        rgbLight.color.copy(c);
      }
      controls.update();
      renderer.render(scene, camera);
    }
    tick();
    controls.addEventListener("start", () => {
      viewer.autoRotate = false;
    });
  }

  function hexColor(h) {
    return new THREE.Color(h || "#f20000");
  }

  function setColors(top, bot) {
    rainbow = false;
    const c1 = hexColor(top);
    const c2 = hexColor(bot);
    viewers.forEach((v) => {
      v.rgbTopMat.color.copy(c1);
      v.rgbBotMat.color.copy(c2);
      v.rgbLight.color.copy(c1);
      v.rgbLight.intensity = c1.getHSL({ h: 0, s: 0, l: 0 }).l > 0.06 ? 1.4 : 0;
    });
  }

  function setRainbow(on, speed) {
    rainbow = !!on;
    if (speed != null) rainbowSpeed = Math.max(0.2, Number(speed) / 100);
  }

  global.OpenPeripheralMic = { mount, setColors, setRainbow };
})(window);
