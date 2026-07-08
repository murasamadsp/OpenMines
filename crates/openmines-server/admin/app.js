let token = localStorage.getItem('admin_token') || '';
document.getElementById('auth-token').value = token;

const API_BASE = '/api';
const ROLE_LABELS = ['Player', 'Moderator', 'Admin'];

// Tab Switching
function switchTab(tabId) {
    document.querySelectorAll('.tab-btn').forEach(btn => btn.classList.remove('active'));
    document.querySelectorAll('.tab-content').forEach(content => content.classList.remove('active'));

    // Find active button
    event.currentTarget.classList.add('active');
    document.getElementById(`tab-${tabId}`).classList.add('active');

    if (tabId === 'map') {
        loadMap();
    } else {
        loadData();
    }
}

// API Helpers
async function apiFetch(url, options = {}) {
    const headers = options.headers || {};
    headers['Authorization'] = `Bearer ${token}`;
    headers['Content-Type'] = 'application/json';

    const response = await fetch(API_BASE + url, { ...options, headers });
    if (response.status === 401) {
        alert('Unauthorized! Please check your token.');
        return null;
    }
    return response.json();
}

async function loadData() {
    const data = await apiFetch('/stats');
    if (!data) return;

    // Update stats counters
    document.getElementById('stat-online').textContent = data.online_count;
    document.getElementById('stat-events-count').textContent = data.active_events.length;

    // Update players list
    const playersBody = document.getElementById('players-table-body');
    playersBody.innerHTML = '';
    data.active_players.forEach(p => {
        const row = document.createElement('tr');
        row.innerHTML = `
            <td>${p.id}</td>
            <td><strong>${p.name}</strong></td>
            <td>${p.x}:${p.y}</td>
            <td>${p.health}/${p.max_health} HP</td>
            <td>$${p.money}</td>
            <td>
                <select onchange="setPlayerRole(${p.id}, this.value)">
                    ${ROLE_LABELS.map((label, role) => `<option value="${role}" ${p.role === role ? 'selected' : ''}>${label}</option>`).join('')}
                </select>
            </td>
            <td>${p.crystals.join(' / ')}</td>
            <td>
                <button class="btn-danger" onclick="kickPlayer(${p.id})">Kick</button>
            </td>
        `;
        playersBody.appendChild(row);
    });

    // Update events list
    const eventsBody = document.getElementById('events-table-body');
    eventsBody.innerHTML = '';
    data.active_events.forEach(e => {
        const row = document.createElement('tr');
        row.innerHTML = `
            <td><code>${e.id}</code></td>
            <td>${e.title}</td>
            <td>${new Date(e.starts_at * 1000).toLocaleString()}</td>
            <td>${new Date(e.ends_at * 1000).toLocaleString()}</td>
            <td>${e.xp_mult}x</td>
            <td>${e.drop_mult}x</td>
            <td>
                <button class="btn-danger" onclick="deleteEvent('${e.id}')">Delete</button>
            </td>
        `;
        eventsBody.appendChild(row);
    });

    // Update market rates table
    const crystalNames = ['Green', 'Blue', 'Red', 'Violet', 'White', 'Cyan'];
    const ratesBody = document.getElementById('market-rates-body');
    ratesBody.innerHTML = '';
    for (let i = 0; i < 6; i++) {
        const row = document.createElement('tr');
        const mod = data.cost_mod[i];
        const sell = data.market_prices[i];
        const buy = sell * 10;

        row.innerHTML = `
            <td><span class="crystal-badge cry-${i}">${crystalNames[i]}</span></td>
            <td>+${mod}</td>
            <td><strong>${sell}</strong></td>
            <td><strong>${buy}</strong></td>
        `;
        ratesBody.appendChild(row);

        // Pre-fill modifier inputs
        document.getElementById(`mod-${i}`).value = mod;
    }
}

async function setPlayerRole(id, role) {
    const res = await apiFetch(`/players/${id}/role`, {
        method: 'POST',
        body: JSON.stringify({ role: parseInt(role) })
    });
    if (!res || !res.success) {
        loadData();
    }
}

async function kickPlayer(id) {
    if (!confirm(`Are you sure you want to kick player ${id}?`)) return;
    const res = await apiFetch(`/players/${id}/kick`, { method: 'POST' });
    if (res && res.success) {
        loadData();
    }
}

async function deleteEvent(id) {
    if (!confirm(`Are you sure you want to delete event ${id}?`)) return;
    const res = await apiFetch(`/events/${id}`, { method: 'DELETE' });
    if (res && res.success) {
        loadData();
    }
}

async function saveEvent(event) {
    event.preventDefault();
    const payload = {
        id: document.getElementById('event-id').value,
        title: document.getElementById('event-title').value,
        starts_at: parseInt(document.getElementById('event-start').value),
        ends_at: parseInt(document.getElementById('event-end').value),
        xp_mult: parseFloat(document.getElementById('event-xp-mult').value),
        drop_mult: parseFloat(document.getElementById('event-drop-mult').value),
    };

    const res = await apiFetch('/events', {
        method: 'POST',
        body: JSON.stringify(payload)
    });
    if (res && res.success) {
        alert('Event saved successfully!');
        loadData();
    }
}

async function saveMarket(event) {
    event.preventDefault();
    const cost_mod = [];
    for (let i = 0; i < 6; i++) {
        cost_mod.push(parseInt(document.getElementById(`mod-${i}`).value));
    }

    const res = await apiFetch('/market', {
        method: 'POST',
        body: JSON.stringify({ cost_mod })
    });
    if (res && res.success) {
        alert('Market modifiers updated successfully!');
        loadData();
    }
}

// Map Rendering logic
async function loadMap() {
    const data = await apiFetch('/map');
    if (!data) return;

    const canvas = document.getElementById('map-canvas');
    const ctx = canvas.getContext('2d');

    // Clear canvas
    ctx.fillStyle = '#0b0d19';
    ctx.fillRect(0, 0, canvas.width, canvas.height);

    // Draw grid
    ctx.strokeStyle = '#1e223b';
    ctx.lineWidth = 0.5;
    const step = 20;
    for (let x = 0; x < canvas.width; x += step) {
        ctx.beginPath();
        ctx.moveTo(x, 0);
        ctx.lineTo(x, canvas.height);
        ctx.stroke();
    }
    for (let y = 0; y < canvas.height; y += step) {
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(canvas.width, y);
        ctx.stroke();
    }

    // Normalize coordinates to fit map viewport
    const scaleX = canvas.width / Math.max(data.width * 32, 100);
    const scaleY = canvas.height / Math.max(data.height * 32, 100);

    // Draw buildings
    data.buildings.forEach(b => {
        ctx.fillStyle = b.pack_type === 'Gun' ? '#da373c' : '#a5b4fc';
        ctx.fillRect(b.x * scaleX, b.y * scaleY, 12, 12);
        ctx.fillStyle = 'white';
        ctx.font = '8px sans-serif';
        ctx.fillText(b.pack_type[0] || 'B', b.x * scaleX + 2, b.y * scaleY + 9);
    });

    // Draw players
    data.players.forEach(p => {
        ctx.fillStyle = '#23a55a';
        ctx.beginPath();
        ctx.arc(p.x * scaleX + 6, p.y * scaleY + 6, 6, 0, 2 * Math.PI);
        ctx.fill();

        ctx.fillStyle = '#f2f3f5';
        ctx.font = 'bold 9px sans-serif';
        ctx.fillText(p.name, p.x * scaleX - 10, p.y * scaleY - 4);
    });
}

// Authentication login
document.getElementById('btn-login').addEventListener('click', async () => {
    const val = document.getElementById('auth-token').value;
    const response = await fetch(`${API_BASE}/auth`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ token: val })
    });
    const result = await response.json();
    if (result && result.success) {
        token = val;
        localStorage.setItem('admin_token', token);
        alert('Authenticated successfully!');
        loadData();
    } else {
        alert('Invalid Token!');
    }
});

// Populate current timestamps for event forms
document.getElementById('event-start').value = Math.floor(Date.now() / 1000);
document.getElementById('event-end').value = Math.floor(Date.now() / 1000) + 86400 * 7; // 1 week

// Auto load
loadData();
setInterval(loadData, 5000);
