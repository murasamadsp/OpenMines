const API_BASE = '/api';
const ROLE_LABELS = ['Player', 'Moderator', 'Admin'];
const CRYSTALS = [
    { name: 'Green', css: 'var(--green)' },
    { name: 'Blue', css: 'var(--blue)' },
    { name: 'Red', css: 'var(--red)' },
    { name: 'Violet', css: 'var(--violet)' },
    { name: 'White', css: 'var(--white)' },
    { name: 'Cyan', css: 'var(--cyan)' },
];

let token = localStorage.getItem('admin_token') || '';
let currentView = 'overview';
let statsData = null;
let mapData = null;
let commandsData = null;
let refreshTimer = null;
let requestInFlight = false;

const mapState = {
    zoom: 1,
    offsetX: 0,
    offsetY: 0,
    dragging: false,
    lastX: 0,
    lastY: 0,
};

const el = (id) => document.getElementById(id);

function setStatus(kind, text) {
    const dot = el('status-dot');
    dot.classList.toggle('ok', kind === 'ok');
    dot.classList.toggle('bad', kind === 'bad');
    el('status-text').textContent = text;
}

function toast(message, kind = '') {
    const item = document.createElement('div');
    item.className = `toast-item ${kind}`.trim();
    item.textContent = message;
    el('toast').appendChild(item);
    window.setTimeout(() => item.remove(), 4200);
}

function textCell(row, value, className = '') {
    const cell = document.createElement('td');
    if (className) cell.className = className;
    cell.textContent = value ?? '';
    row.appendChild(cell);
    return cell;
}

function clear(node) {
    node.replaceChildren();
}

function emptyRow(tbody, columns, message) {
    const row = document.createElement('tr');
    const cell = textCell(row, message, 'empty');
    cell.colSpan = columns;
    tbody.appendChild(row);
}

async function readResponse(response) {
    const type = response.headers.get('content-type') || '';
    if (type.includes('application/json')) {
        return response.json();
    }
    return response.text();
}

async function apiFetch(path, options = {}) {
    const headers = new Headers(options.headers || {});
    headers.set('Authorization', `Bearer ${token}`);
    if (options.body !== undefined) {
        headers.set('Content-Type', 'application/json');
    }

    let response;
    try {
        response = await fetch(API_BASE + path, { ...options, headers });
    } catch (error) {
        setStatus('bad', `Network error: ${error.message}`);
        toast(`Network error: ${error.message}`, 'bad');
        return null;
    }

    const body = await readResponse(response);
    if (response.status === 401) {
        setStatus('bad', 'Unauthorized');
        toast('Invalid or missing admin token', 'bad');
        return null;
    }
    if (!response.ok) {
        const message = typeof body === 'object' && body !== null && body.message
            ? body.message
            : `${response.status} ${response.statusText}`;
        setStatus('bad', message);
        toast(message, 'bad');
        return null;
    }
    return body;
}

async function authenticate() {
    const value = el('auth-token').value.trim();
    const response = await fetch(`${API_BASE}/auth`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ token: value }),
    });
    const body = await readResponse(response);
    if (response.ok && body && body.success) {
        token = value;
        localStorage.setItem('admin_token', token);
        setStatus('ok', 'Authenticated');
        toast('Authenticated', 'ok');
        await refreshAll();
    } else {
        setStatus('bad', 'Invalid token');
        toast('Invalid token', 'bad');
    }
}

async function refreshAll() {
    if (!token || requestInFlight) return;
    requestInFlight = true;
    try {
        const data = await apiFetch('/stats');
        if (!data) return;
        statsData = data;
        renderStats();
        if (currentView === 'map' || !mapData) {
            await loadMap();
        }
        setStatus('ok', `Updated ${new Date().toLocaleTimeString()}`);
    } finally {
        requestInFlight = false;
    }
}

async function loadMap() {
    const data = await apiFetch('/map');
    if (!data) return;
    mapData = data;
    el('metric-buildings').textContent = data.buildings.length.toString();
    el('metric-world').textContent = `${data.width}x${data.height}`;
    el('map-player-count').textContent = data.players.length.toString();
    el('map-building-count').textContent = data.buildings.length.toString();
    el('map-world').textContent = `${data.width}x${data.height}`;
    if (currentView === 'map') {
        renderMap();
    }
}

async function loadCommands() {
    if (commandsData) {
        renderCommands();
        return;
    }
    const data = await apiFetch('/admin/commands');
    if (!data) return;
    commandsData = data;
    renderCommands();
}

function renderStats() {
    if (!statsData) return;
    el('metric-online').textContent = statsData.online_count.toString();
    el('metric-events').textContent = statsData.active_events.length.toString();
    el('players-count').textContent = `${statsData.active_players.length} rows`;
    renderOverviewPlayers();
    renderPlayers();
    renderEvents();
    renderMarket();
}

function renderOverviewPlayers() {
    const tbody = el('overview-players');
    clear(tbody);
    const players = [...statsData.active_players].sort((a, b) => a.id - b.id).slice(0, 12);
    if (players.length === 0) {
        emptyRow(tbody, 6, 'No online players');
        return;
    }
    for (const player of players) {
        const row = document.createElement('tr');
        textCell(row, player.id);
        textCell(row, player.name);
        textCell(row, `${player.x}:${player.y}`);
        textCell(row, `${player.health}/${player.max_health}`);
        textCell(row, player.money);
        textCell(row, ROLE_LABELS[player.role] || `Role ${player.role}`);
        tbody.appendChild(row);
    }
}

function filteredPlayers() {
    const query = el('player-filter').value.trim().toLowerCase();
    const players = [...(statsData?.active_players || [])].sort((a, b) => a.id - b.id);
    if (!query) return players;
    return players.filter((p) => String(p.id).includes(query) || p.name.toLowerCase().includes(query));
}

function renderPlayers() {
    const tbody = el('players-table');
    clear(tbody);
    const players = filteredPlayers();
    if (players.length === 0) {
        emptyRow(tbody, 9, 'No matching online players');
        return;
    }
    for (const player of players) {
        const row = document.createElement('tr');
        textCell(row, player.id);
        textCell(row, player.name);
        textCell(row, `${player.x}:${player.y}`);
        textCell(row, `${player.health}/${player.max_health}`);
        textCell(row, player.money);
        textCell(row, player.creds);
        textCell(row, player.crystals.join(' / '));

        const roleCell = document.createElement('td');
        const select = document.createElement('select');
        ROLE_LABELS.forEach((label, role) => {
            const option = document.createElement('option');
            option.value = role.toString();
            option.textContent = label;
            option.selected = player.role === role;
            select.appendChild(option);
        });
        select.addEventListener('change', () => setPlayerRole(player.id, Number(select.value)));
        roleCell.appendChild(select);
        row.appendChild(roleCell);

        const actions = document.createElement('td');
        const kick = document.createElement('button');
        kick.className = 'btn-danger';
        kick.textContent = 'Kick';
        kick.addEventListener('click', () => kickPlayer(player.id, player.name));
        actions.appendChild(kick);
        row.appendChild(actions);

        tbody.appendChild(row);
    }
}

function renderEvents() {
    const tbody = el('events-table');
    clear(tbody);
    const events = [...statsData.active_events].sort((a, b) => a.starts_at - b.starts_at);
    if (events.length === 0) {
        emptyRow(tbody, 7, 'No active or scheduled events');
        return;
    }
    for (const item of events) {
        const row = document.createElement('tr');
        textCell(row, item.id);
        textCell(row, item.title);
        textCell(row, formatUnix(item.starts_at));
        textCell(row, formatUnix(item.ends_at));
        textCell(row, `${item.xp_mult}x`);
        textCell(row, `${item.drop_mult}x`);

        const actions = document.createElement('td');
        const edit = document.createElement('button');
        edit.textContent = 'Edit';
        edit.addEventListener('click', () => fillEventForm(item));
        const del = document.createElement('button');
        del.className = 'btn-danger';
        del.textContent = 'Delete';
        del.addEventListener('click', () => deleteEvent(item.id));
        actions.append(edit, ' ', del);
        row.appendChild(actions);
        tbody.appendChild(row);
    }
}

function renderMarket() {
    renderMarketTable(el('market-table'));
    renderMarketTable(el('overview-market'));
    if (document.activeElement && document.activeElement.closest('#market-form')) return;
    for (let i = 0; i < 6; i += 1) {
        el(`mod-${i}`).value = statsData.cost_mod[i] ?? '';
    }
}

function renderMarketTable(tbody) {
    clear(tbody);
    for (let i = 0; i < CRYSTALS.length; i += 1) {
        const row = document.createElement('tr');
        const nameCell = document.createElement('td');
        const swatch = document.createElement('span');
        swatch.className = 'swatch';
        swatch.style.setProperty('--swatch', CRYSTALS[i].css);
        swatch.textContent = CRYSTALS[i].name;
        nameCell.appendChild(swatch);
        row.appendChild(nameCell);
        textCell(row, statsData.cost_mod[i]);
        textCell(row, statsData.market_prices[i]);
        textCell(row, Number(statsData.market_prices[i]) * 10);
        tbody.appendChild(row);
    }
}

function renderCommands() {
    const list = el('commands-list');
    clear(list);
    if (!commandsData || commandsData.length === 0) {
        const empty = document.createElement('div');
        empty.className = 'empty';
        empty.textContent = 'No command specs';
        list.appendChild(empty);
        return;
    }
    for (const command of commandsData) {
        const card = document.createElement('div');
        card.className = 'command';

        const title = document.createElement('div');
        title.className = 'command-title';
        const name = document.createElement('strong');
        name.textContent = command.name;
        const description = document.createElement('span');
        description.className = 'muted';
        description.textContent = command.description;
        title.append(name, description);

        const slash = document.createElement('code');
        slash.textContent = command.slash || 'no slash command';
        const consoleCommand = document.createElement('code');
        consoleCommand.textContent = command.console || 'no console command';

        card.append(title, slash, consoleCommand);
        list.appendChild(card);
    }
}

function formatUnix(value) {
    if (!Number.isFinite(Number(value))) return '-';
    return new Date(Number(value) * 1000).toLocaleString();
}

function fillEventForm(item) {
    el('event-id').value = item.id;
    el('event-title').value = item.title;
    el('event-start').value = item.starts_at;
    el('event-end').value = item.ends_at;
    el('event-xp-mult').value = item.xp_mult;
    el('event-drop-mult').value = item.drop_mult;
    switchView('events');
}

function fillDefaultEventDates() {
    const now = Math.floor(Date.now() / 1000);
    el('event-start').value = now;
    el('event-end').value = now + 86400 * 7;
}

async function setPlayerRole(id, role) {
    const result = await apiFetch(`/players/${id}/role`, {
        method: 'POST',
        body: JSON.stringify({ role }),
    });
    if (result && result.success) {
        toast(`Role updated for ${id}`, 'ok');
    }
    await refreshAll();
}

async function kickPlayer(id, name) {
    const label = name ? `${name} (${id})` : id;
    if (!window.confirm(`Kick ${label}?`)) return;
    const result = await apiFetch(`/players/${id}/kick`, { method: 'POST' });
    if (result && result.success) {
        toast(`Kicked ${label}`, 'ok');
    }
    await refreshAll();
}

async function deleteEvent(id) {
    if (!window.confirm(`Delete event ${id}?`)) return;
    const result = await apiFetch(`/events/${encodeURIComponent(id)}`, { method: 'DELETE' });
    if (result && result.success) {
        toast(`Event ${id} deleted`, 'ok');
    }
    await refreshAll();
}

async function saveEvent(event) {
    event.preventDefault();
    const payload = {
        id: el('event-id').value.trim(),
        title: el('event-title').value.trim(),
        starts_at: Number.parseInt(el('event-start').value, 10),
        ends_at: Number.parseInt(el('event-end').value, 10),
        xp_mult: Number.parseFloat(el('event-xp-mult').value),
        drop_mult: Number.parseFloat(el('event-drop-mult').value),
    };
    const result = await apiFetch('/events', {
        method: 'POST',
        body: JSON.stringify(payload),
    });
    if (result && result.success) {
        toast(`Event ${payload.id} saved`, 'ok');
        await refreshAll();
    }
}

async function saveMarket(event) {
    event.preventDefault();
    const cost_mod = [];
    for (let i = 0; i < 6; i += 1) {
        cost_mod.push(Number.parseInt(el(`mod-${i}`).value, 10));
    }
    const result = await apiFetch('/market', {
        method: 'POST',
        body: JSON.stringify({ cost_mod }),
    });
    if (result && result.success) {
        toast('Market updated', 'ok');
        await refreshAll();
    }
}

function switchView(view) {
    currentView = view;
    document.querySelectorAll('[data-view-button]').forEach((button) => {
        button.classList.toggle('active', button.dataset.viewButton === view);
    });
    document.querySelectorAll('.view').forEach((section) => {
        section.classList.toggle('active', section.id === `view-${view}`);
    });
    if (view === 'map') loadMap();
    if (view === 'commands') loadCommands();
}

function setupAutoRefresh() {
    if (refreshTimer) {
        window.clearInterval(refreshTimer);
        refreshTimer = null;
    }
    if (el('auto-refresh').checked) {
        refreshTimer = window.setInterval(refreshAll, 5000);
    }
}

function setupMap() {
    const canvas = el('map-canvas');
    canvas.addEventListener('mousedown', (event) => {
        mapState.dragging = true;
        mapState.lastX = event.clientX;
        mapState.lastY = event.clientY;
    });
    window.addEventListener('mouseup', () => {
        mapState.dragging = false;
    });
    window.addEventListener('mousemove', (event) => {
        if (!mapState.dragging) return;
        mapState.offsetX += event.clientX - mapState.lastX;
        mapState.offsetY += event.clientY - mapState.lastY;
        mapState.lastX = event.clientX;
        mapState.lastY = event.clientY;
        renderMap();
    });
    canvas.addEventListener('wheel', (event) => {
        event.preventDefault();
        zoomMap(event.deltaY < 0 ? 1.15 : 0.87, event.offsetX, event.offsetY);
    }, { passive: false });
    el('map-zoom-in').addEventListener('click', () => zoomMap(1.2));
    el('map-zoom-out').addEventListener('click', () => zoomMap(0.85));
    el('map-reset').addEventListener('click', () => {
        mapState.zoom = 1;
        mapState.offsetX = 0;
        mapState.offsetY = 0;
        renderMap();
    });
}

function zoomMap(factor, anchorX = null, anchorY = null) {
    const canvas = el('map-canvas');
    const before = screenToWorld(canvas, anchorX ?? canvas.clientWidth / 2, anchorY ?? canvas.clientHeight / 2);
    mapState.zoom = Math.min(20, Math.max(0.2, mapState.zoom * factor));
    const after = worldToScreen(canvas, before.x, before.y);
    if (anchorX !== null && anchorY !== null) {
        mapState.offsetX += anchorX - after.x;
        mapState.offsetY += anchorY - after.y;
    }
    renderMap();
}

function resizeCanvas(canvas) {
    const rect = canvas.getBoundingClientRect();
    const ratio = window.devicePixelRatio || 1;
    const width = Math.max(1, Math.floor(rect.width * ratio));
    const height = Math.max(1, Math.floor(rect.height * ratio));
    if (canvas.width !== width || canvas.height !== height) {
        canvas.width = width;
        canvas.height = height;
    }
    return { width, height, ratio };
}

function baseScale(canvas) {
    if (!mapData) return 1;
    return Math.min(canvas.width / Math.max(mapData.width, 1), canvas.height / Math.max(mapData.height, 1)) * 0.92;
}

function worldToScreen(canvas, x, y) {
    const scale = baseScale(canvas) * mapState.zoom;
    return {
        x: x * scale + canvas.width / 2 - (mapData.width * scale) / 2 + mapState.offsetX,
        y: y * scale + canvas.height / 2 - (mapData.height * scale) / 2 + mapState.offsetY,
    };
}

function screenToWorld(canvas, x, y) {
    const ratio = window.devicePixelRatio || 1;
    const sx = x * ratio;
    const sy = y * ratio;
    const scale = baseScale(canvas) * mapState.zoom;
    return {
        x: (sx - canvas.width / 2 + (mapData.width * scale) / 2 - mapState.offsetX) / scale,
        y: (sy - canvas.height / 2 + (mapData.height * scale) / 2 - mapState.offsetY) / scale,
    };
}

function renderMap() {
    if (!mapData) return;
    const canvas = el('map-canvas');
    const ctx = canvas.getContext('2d');
    resizeCanvas(canvas);

    ctx.fillStyle = '#0b0d10';
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    const scale = baseScale(canvas) * mapState.zoom;
    const origin = worldToScreen(canvas, 0, 0);
    const worldW = mapData.width * scale;
    const worldH = mapData.height * scale;

    ctx.strokeStyle = '#2b3036';
    ctx.lineWidth = 1;
    ctx.strokeRect(origin.x, origin.y, worldW, worldH);

    drawGrid(ctx, origin, worldW, worldH, scale);
    drawBuildings(ctx, canvas);
    drawPlayers(ctx, canvas);

    el('map-zoom').textContent = `${mapState.zoom.toFixed(2)}x`;
}

function drawGrid(ctx, origin, worldW, worldH, scale) {
    const step = chooseGridStep(scale);
    const screenStep = step * scale;
    if (screenStep < 8) return;
    ctx.strokeStyle = '#1f2429';
    ctx.lineWidth = 1;
    for (let x = 0; x <= worldW; x += screenStep) {
        ctx.beginPath();
        ctx.moveTo(origin.x + x, origin.y);
        ctx.lineTo(origin.x + x, origin.y + worldH);
        ctx.stroke();
    }
    for (let y = 0; y <= worldH; y += screenStep) {
        ctx.beginPath();
        ctx.moveTo(origin.x, origin.y + y);
        ctx.lineTo(origin.x + worldW, origin.y + y);
        ctx.stroke();
    }
}

function chooseGridStep(scale) {
    if (scale > 10) return 32;
    if (scale > 3) return 128;
    return 512;
}

function drawBuildings(ctx, canvas) {
    for (const building of mapData.buildings) {
        const point = worldToScreen(canvas, building.x, building.y);
        const size = Math.max(4, Math.min(14, 5 * mapState.zoom));
        ctx.fillStyle = building.pack_type === 'Gun' ? '#d65f5f' : '#4da3ff';
        ctx.fillRect(point.x - size / 2, point.y - size / 2, size, size);
    }
}

function drawPlayers(ctx, canvas) {
    for (const player of mapData.players) {
        const point = worldToScreen(canvas, player.x, player.y);
        const radius = Math.max(4, Math.min(10, 4 * mapState.zoom));
        ctx.fillStyle = '#2ea66f';
        ctx.beginPath();
        ctx.arc(point.x, point.y, radius, 0, Math.PI * 2);
        ctx.fill();
        if (mapState.zoom >= 0.65) {
            ctx.fillStyle = '#eceff3';
            ctx.font = `${Math.max(11, Math.min(16, 10 * mapState.zoom))}px system-ui`;
            ctx.fillText(player.name, point.x + radius + 4, point.y - radius);
        }
    }
}

async function loadMetrics() {
    const output = el('metrics-output');
    output.textContent = 'Loading...';
    const response = await fetch('/metrics');
    output.textContent = response.ok ? await response.text() : `${response.status} ${response.statusText}`;
}

function bindUi() {
    el('auth-token').value = token;
    document.querySelectorAll('[data-view-button]').forEach((button) => {
        button.addEventListener('click', () => switchView(button.dataset.viewButton));
    });
    el('btn-login').addEventListener('click', authenticate);
    el('auth-token').addEventListener('keydown', (event) => {
        if (event.key === 'Enter') authenticate();
    });
    el('btn-refresh').addEventListener('click', refreshAll);
    el('auto-refresh').addEventListener('change', setupAutoRefresh);
    el('player-filter').addEventListener('input', renderPlayers);
    el('event-form').addEventListener('submit', saveEvent);
    el('market-form').addEventListener('submit', saveMarket);
    el('event-fill-week').addEventListener('click', fillDefaultEventDates);
    el('btn-load-metrics').addEventListener('click', loadMetrics);
    window.addEventListener('resize', renderMap);
    setupMap();
    setupAutoRefresh();
}

bindUi();
fillDefaultEventDates();
if (token) {
    refreshAll();
} else {
    setStatus('', 'Enter admin token');
}
