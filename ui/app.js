(function () {
  'use strict';
  var t = TeleseraI18n.t, locale = TeleseraI18n.locale;
  TeleseraI18n.apply(document);
  var $ = function (id) { return document.getElementById(id); };
  var token = new URLSearchParams(location.hash.slice(1)).get('token');
  if (token) { sessionStorage.setItem('telesera-token', token); history.replaceState(null, '', location.pathname + location.search); }
  token = token || sessionStorage.getItem('telesera-token') || '';
  var state = { chats: [], chat: null, archived: false, messages: [], hasMore: false, authStep: 'phone', loading: false, videoKey: '', returnFocus: null, miniOrigin: '', miniBack: false, lastActivity: Date.now(), lastInput: Date.now(), lastProgress: 0, lastTime: 0 };
  var toastTimer, chatRequest = 0, lastChatsPayload = null, liveRun = 0;
  var miniTheme = { bg_color: '#172238', text_color: '#e7edf8', hint_color: '#a0aec5', link_color: '#aac8ff', button_color: '#6389db', button_text_color: '#ffffff', secondary_bg_color: '#1b2941' };
  function normalize(value) { return String(value || '').normalize('NFKC'); }
  function el(tag, className, text) { var node = document.createElement(tag); if (className) node.className = className; if (text !== undefined) node.textContent = text; return node; }
  function show(node, visible) { node.hidden = !visible; }
  function toast(message) { $('toast').textContent = message; show($('toast'), true); clearTimeout(toastTimer); toastTimer = setTimeout(function () { show($('toast'), false); }, 6000); }
  function apiError(message, status) {
    var raw = String(message || '');
    if (/FLOOD|SLOWMODE/i.test(raw)) return t('Telegram is limiting requests. Please wait before trying again.');
    if (/PHONE_NUMBER_INVALID|Numero non valido/i.test(raw)) return t('Enter a valid phone number.');
    if (/PHONE_CODE_|Richiedi prima il codice|invalid.*code|code.*invalid/i.test(raw)) return t('The login code is invalid or expired. Request a new code.');
    if (/PASSWORD_HASH_INVALID|password.*incorrect|invalid.*password/i.test(raw)) return t('The password is incorrect. Please try again.');
    if (/CHAT_WRITE_FORBIDDEN|USER_BANNED_IN_CHANNEL|CHAT_SEND_STICKERS_FORBIDDEN|CHAT_SEND_MEDIA_FORBIDDEN/i.test(raw)) return t('You cannot send messages in this chat.');
    if (/4096/.test(raw)) return t('Messages must contain 1 to 4096 characters.');
    if (/live stream/i.test(raw)) return t('The live stream has ended or is not available.');
    if (/Refresh the sticker picker|FILE_REFERENCE_EXPIRED|STICKER_INVALID/i.test(raw)) return t('Reopen the sticker picker and try again.');
    if (status === 401 || /AUTH_KEY_UNREGISTERED|SESSION_REVOKED/i.test(raw)) return t('Sign in again to continue.');
    return t('The operation failed. Please try again.');
  }
  async function api(path, data) {
    var controller = new AbortController();
    var options = { headers: { Authorization: 'Bearer ' + token }, cache: 'no-store', signal: controller.signal };
    if (data !== undefined) { options.method = 'POST'; options.headers['Content-Type'] = 'application/json'; options.body = JSON.stringify(data); }
    var timeout = setTimeout(function () { controller.abort(); }, 25000);
    var response;
    try {
      response = await fetch('/api' + path, options);
      var body;
      try { body = await response.json(); } catch (error) { if (controller.signal.aborted) throw error; throw new Error(t("The service is not ready yet. Try again in a moment.")); }
      if (!response.ok) throw new Error(apiError(body.error, response.status));
      return body;
    } catch (error) {
      if (controller.signal.aborted) throw new Error(t("The response is taking too long. Check your connection and try again."));
      if (!response) throw new Error(t("The connection was interrupted. Try again in a moment."));
      throw error;
    } finally { clearTimeout(timeout); }
  }
  function resource(path) { return '/api' + path + '?token=' + encodeURIComponent(token); }
  function chatPath(chat) { return '/chats/' + encodeURIComponent(chat.id); }
  function mediaPath(chat, message) { return chatPath(chat) + '/messages/' + message.id; }
  function initials(name) { return normalize(name).trim().split(/\s+/).slice(0, 2).map(function (part) { return Array.from(part)[0] || ''; }).join('').toUpperCase() || '·'; }
  var avatarFailures = new Map();
  function loadAvatar(node) {
    var entry = avatarFailures.get(node.dataset.photo), now = Date.now();
    if (node.querySelector('img') || entry && entry.next > now) return;
    var attempt = (entry ? entry.attempts : 0) + 1;
    var img = el('img'); img.alt = ''; img.loading = 'lazy'; img.decoding = 'async';
    img.onload = function () { node.firstChild.nodeValue = ''; avatarFailures.delete(node.dataset.photo); };
    img.onerror = function () {
      img.remove(); node.firstChild.nodeValue = node.dataset.initials;
      var failed = avatarFailures.get(node.dataset.photo) || { attempts: 0 };
      failed.attempts = Math.max(failed.attempts, attempt); failed.next = Date.now() + Math.min(300000, 30000 * Math.pow(2, failed.attempts - 1));
      avatarFailures.set(node.dataset.photo, failed);
    };
    img.src = node.dataset.photo + (entry ? '&retry=' + entry.attempts : ''); node.appendChild(img);
  }
  function retryAvatars() { document.querySelectorAll('.avatar[data-photo]').forEach(loadAvatar); }
  function avatar(chat, small) {
    var node = el('span', 'avatar' + (small ? ' small' : ''), initials(chat.title));
    node.dataset.initials = initials(chat.title);
    var hash = 0; for (var i = 0; i < chat.title.length; i++) hash = ((hash << 5) - hash + chat.title.charCodeAt(i)) | 0;
    var hue = Math.abs(hash) % 80 + 190; node.style.backgroundColor = 'hsl(' + hue + ',55%,90%)'; node.style.color = 'hsl(' + hue + ',35%,40%)';
    if (chat.photo) { node.dataset.photo = resource(chatPath(chat) + '/photo'); loadAvatar(node); }
    return node;
  }
  function time(date) { return new Date(date).toLocaleTimeString(locale, { hour: '2-digit', minute: '2-digit' }); }
  function duration(seconds) { var value = Math.max(0, Math.floor(Number(seconds) || 0)); return (value >= 3600 ? Math.floor(value / 3600) + ':' : '') + (value >= 3600 ? String(Math.floor(value % 3600 / 60)).padStart(2, '0') : Math.floor(value / 60)) + ':' + String(value % 60).padStart(2, '0'); }
  function rememberFocus(container) { var active = document.activeElement; return active && container.contains(active) ? active.dataset.focusKey : null; }
  function restoreFocus(container, key) { if (!key) return; var nodes = container.querySelectorAll('[data-focus-key]'); for (var i = 0; i < nodes.length; i++) if (nodes[i].dataset.focusKey === key) { nodes[i].focus({ preventScroll: true }); break; } }
  function setFocus(node) { if (node && !node.hidden) node.focus(); }
  var connecting = false, reconnectTimer;
  async function connect() {
    if (connecting) return;
    connecting = true; clearTimeout(reconnectTimer);
    show($('retry-connection'), false); $('connection-message').textContent = t("Connecting to your account…");
    try {
      if (!token) throw new Error(t("Open Telesera from the TV app to connect to your account."));
      var status = await api('/status');
      if (status.authorized) { show($('connection'), false); show($('login'), false); show($('app'), true); $('account-name').textContent = normalize(status.user && status.user.name || t("Your account")); $('account-avatar').textContent = initials($('account-name').textContent); await loadChats(); setFocus($('chat-list').querySelector('button') || $('inbox-tab')); }
      else { show($('connection'), false); show($('login'), true); renderLogin(); }
    } catch (error) {
      $('connection-message').textContent = error.message; show($('retry-connection'), true);
      if (token && !$('connection').hidden) reconnectTimer = setTimeout(connect, 5000);
    } finally { connecting = false; }
  }
  function renderLogin() {
    var step = state.authStep, input = $('login-input'); input.value = ''; show($('login-error'), false);
    $('login-label').textContent = step === 'phone' ? t("Phone number") : step === 'code' ? t("Login code") : t("Two-step verification password");
    input.type = step === 'phone' ? 'tel' : step === 'password' ? 'password' : 'text'; input.inputMode = step === 'password' ? 'text' : 'numeric'; input.autocomplete = step === 'phone' ? 'tel' : step === 'code' ? 'one-time-code' : 'current-password'; input.placeholder = step === 'phone' ? '+39 333 1234567' : step === 'code' ? t("Telegram code") : t("Your password");
    $('login-help').textContent = step === 'phone' ? t("Enter your Telegram phone number, including the country code.") : step === 'code' ? t("Enter the code received in Telegram or by SMS.") : t("Your account is protected by an additional password.");
    $('login-next').textContent = step === 'phone' ? t("Continue") : t("Sign in"); show($('login-back'), step !== 'phone'); input.focus();
  }
  async function login() {
    var value = $('login-input').value.trim(); if (!value) { $('login-input').focus(); return; }
    var key = state.authStep, payload = {}; payload[key] = key === 'password' ? $('login-input').value : value;
    $('login-next').disabled = true; show($('login-error'), false);
    try { var result = await api('/auth/' + key, payload); $('login-input').value = ''; if (result.step === 'ready') await connect(); else { state.authStep = result.step; renderLogin(); } }
    catch (error) { $('login-error').textContent = error.message; show($('login-error'), true); }
    finally { $('login-next').disabled = false; }
  }
  function renderChats() {
    var container = $('chat-list'), focus = rememberFocus(container), scroll = container.scrollTop, query = normalize($('search').value).toLocaleLowerCase(locale);
    container.textContent = ''; $('archive-count').textContent = state.chats.filter(function (chat) { return chat.archived; }).length || '';
    var chats = state.chats.filter(function (chat) { return Boolean(chat.archived) === state.archived && normalize(chat.title).toLocaleLowerCase(locale).includes(query); });
    chats.forEach(function (chat) {
      var row = el('button', 'chat-row' + (state.chat && state.chat.id === chat.id ? ' selected' : '')); row.dataset.focusKey = 'chat-' + chat.id; row.setAttribute('aria-label', normalize(chat.title) + (chat.unread ? ", " + t("{count} unread messages", { count: chat.unread }) : '')); row.setAttribute('aria-current', state.chat && state.chat.id === chat.id ? 'true' : 'false'); row.appendChild(avatar(chat));
      var content = el('span', 'chat-row-content'); content.appendChild(el('span', 'chat-row-title', normalize(chat.title))); content.appendChild(el('span', 'chat-preview', normalize(chat.last_text || ({ bot: t("Bot"), channel: t("Channel"), group: t("Group"), saved: t("Saved Messages") }[chat.kind] || t("Open conversation"))))); row.appendChild(content); if (chat.live) row.appendChild(el('span', 'live-badge', t("Live"))); if (chat.unread) row.appendChild(el('span', 'unread', chat.unread > 99 ? '99+' : chat.unread)); row.addEventListener('click', function () { openChat(chat); }); container.appendChild(row);
    });
    if (state.chat) { var current = state.chats.find(function (chat) { return chat.id === state.chat.id; }); show($('watch-live'), Boolean(current && current.live)); }
    if (!chats.length) container.appendChild(el('p', 'list-empty', query ? t("No chats found.") : state.archived ? t("No archived chats.") : t("Your chats will appear here.")));
    container.scrollTop = scroll; restoreFocus(container, focus);
  }
  async function loadChats() { try { var result = await api('/chats'), payload = JSON.stringify(result.chats); if (payload !== lastChatsPayload) { state.chats = result.chats; lastChatsPayload = payload; renderChats(); } retryAvatars(); } catch (error) { if (!state.chats.length) { lastChatsPayload = null; $('chat-list').textContent = ''; $('chat-list').appendChild(el('p', 'list-empty', error.message)); var retry = el('button', 'pill load-older', t("Retry")); retry.onclick = loadChats; $('chat-list').appendChild(retry); } else toast(error.message); } }
  function selectArchive(archived) { state.archived = archived; $('inbox-tab').classList.toggle('selected', !archived); $('archive-tab').classList.toggle('selected', archived); $('inbox-tab').setAttribute('aria-pressed', String(!archived)); $('archive-tab').setAttribute('aria-pressed', String(archived)); renderChats(); }
  async function openChat(chat) {
    closeStickers(false); state.chat = chat; state.messages = []; state.hasMore = false; chatRequest++; state.loading = false;
    $('app').classList.add('chat-open'); show($('welcome'), false); show($('conversation-head'), true); show($('message-list'), true); show($('composer'), true); show($('chat-error'), false); $('message-list').textContent = ''; $('message-list').appendChild(el('p', 'list-empty', t("Loading…"))); $('chat-title').textContent = normalize(chat.title); $('chat-kind').textContent = { bot: t("Bot"), user: t("Conversation"), group: t("Group"), channel: t("Channel"), saved: t("Saved Messages") }[chat.kind] || t("Conversation"); $('chat-avatar').textContent = ''; $('chat-avatar').appendChild(avatar(chat)); show($('bot-app'), chat.kind === 'bot' && Boolean(chat.username)); show($('watch-live'), Boolean(chat.live)); $('message-input').value = ''; renderChats(); await loadMessages(false, true);
    if (state.chat && state.chat.id === chat.id) {
      var messages = $('message-list').querySelectorAll('.message'), last = messages[messages.length - 1];
      var actions = last ? visibleFocusable(last) : [];
      // Keep the latest message in view; focusing history or the composer jumps away or opens the TV keyboard.
      (actions[actions.length - 1] || last || $('sticker-button')).focus({ preventScroll: true });
    }
  }
  function appendText(node, text) {
    var parts = String(text).split(/(https?:\/\/[^\s<>]+)/g); parts.forEach(function (part) { if (!/^https?:\/\//.test(part)) { node.appendChild(document.createTextNode(part)); return; } var link = el('a', '', part); link.href = part; link.rel = 'noopener noreferrer'; link.target = '_blank'; link.addEventListener('click', function (event) { event.preventDefault(); openLink(part); }); node.appendChild(link); });
  }
  function messageNode(message, chat) {
    var article = el('article', 'message' + (message.outgoing ? ' outgoing' : '')); article.dataset.messageId = message.id;
    if (message.media) {
      var media = message.media;
      if (media.kind === 'video') { var card = el('button', 'video-card'); card.dataset.focusKey = 'video-' + message.id; card.setAttribute('aria-label', t("Play {name}", { name: media.name || t("Video") })); var poster = el('img'); poster.alt = ''; poster.loading = 'lazy'; poster.src = resource(mediaPath(chat, message) + '/thumbnail'); poster.onerror = function () { poster.remove(); }; card.appendChild(poster); card.appendChild(el('span', 'video-play', '▶')); card.appendChild(el('span', 'video-label', (media.name || t("Video")) + (media.duration ? ' · ' + duration(media.duration) : ''))); card.onclick = function () { openVideo(message, chat); }; article.appendChild(card); }
      else if (media.kind === 'sticker') { var sticker = el('span', 'message-sticker', media.emoji || t('Sticker')); var picture = el('img'); picture.alt = media.emoji || t('Sticker'); picture.loading = 'lazy'; picture.src = resource(mediaPath(chat, message) + '/thumbnail'); picture.onload = function () { sticker.firstChild.nodeValue = ''; }; picture.onerror = function () { picture.remove(); }; sticker.appendChild(picture); article.appendChild(sticker); }
      else if (media.kind === 'photo') { var photo = el('img', 'media-photo'); photo.alt = message.text || t("Photo"); photo.loading = 'lazy'; photo.src = resource(mediaPath(chat, message) + '/media'); photo.onerror = function () { photo.replaceWith(el('p', 'error-text', t("Could not load the photo. Reopen the chat to retry."))); }; article.appendChild(photo); }
      else { var doc = el('a', 'document-link', '↓ ' + (media.name || t("Document"))); doc.href = resource(mediaPath(chat, message) + '/media'); doc.download = media.name || t("Document"); doc.dataset.focusKey = 'doc-' + message.id; article.appendChild(doc); }
    }
    if (message.text) { var textNode = el('div', 'message-text'); appendText(textNode, message.text); article.appendChild(textNode); }
    if (message.buttons && message.buttons.length) {
      var buttons = el('div', 'bot-buttons'); message.buttons.forEach(function (row, rowIndex) { var rowNode = el('div', 'bot-row'); row.forEach(function (button, colIndex) { var node = el('button', '', normalize(button.text)); node.dataset.focusKey = 'button-' + message.id + '-' + rowIndex + '-' + colIndex; node.disabled = button.kind === 'unsupported'; if (node.disabled) node.title = t("This action is not available on the TV"); node.onclick = function () { pressBotButton(message, chat, button, rowIndex, colIndex, node); }; rowNode.appendChild(node); }); buttons.appendChild(rowNode); }); article.appendChild(buttons);
    }
    var stamp = el('div', 'message-time', time(message.date)); article.appendChild(stamp);
    var actions = article.querySelectorAll('button:not(:disabled),a[href]');
    actions.forEach(function (node, index) { if (!node.dataset.focusKey) node.dataset.focusKey = 'action-' + message.id + '-' + index; });
    if (!actions.length) { article.tabIndex = 0; article.dataset.focusKey = 'message-' + message.id; } return article;
  }
  function renderMessages(forceBottom) {
    var container = $('message-list'), focus = rememberFocus(container), oldHeight = container.scrollHeight, oldTop = container.scrollTop, nearBottom = oldHeight - oldTop - container.clientHeight < 90;
    var previousNodes = {}; Array.prototype.forEach.call(container.querySelectorAll('article[data-message-id]'), function (node) { previousNodes[node.dataset.messageId] = node; });
    var fragment = document.createDocumentFragment(); if (state.hasMore) { var older = el('button', 'pill load-older', t("Earlier messages")); older.dataset.focusKey = 'older'; older.onclick = function () { loadMessages(true); }; fragment.appendChild(older); }
    var lastDay = ''; state.messages.forEach(function (message) { var day = new Date(message.date).toLocaleDateString(locale, { day: 'numeric', month: 'long' }); if (day !== lastDay) { fragment.appendChild(el('div', 'day-label', day)); lastDay = day; } var signature = JSON.stringify(message), previous = previousNodes[message.id]; var node = previous && previous.dataset.signature === signature ? previous : messageNode(message, state.chat); node.dataset.signature = signature; fragment.appendChild(node); });
    if (!state.messages.length) fragment.appendChild(el('p', 'list-empty', t("No messages yet.")));
    container.textContent = ''; container.appendChild(fragment);
    if (forceBottom || nearBottom) container.scrollTop = container.scrollHeight; else container.scrollTop = oldTop;
    restoreFocus(container, focus);
  }
  async function loadMessages(older, forceBottom) {
    if (!state.chat || state.loading) return; state.loading = true; var chat = state.chat, request = chatRequest, before = older && state.messages.length ? state.messages[0].id : null;
    try {
      var result = await api(chatPath(chat) + '/messages' + (before ? '?before=' + before : '')); if (request !== chatRequest) return;
      var oldHeight = $('message-list').scrollHeight, oldTop = $('message-list').scrollTop, merged = {};
      state.messages.forEach(function (message) { merged[message.id] = message; }); result.messages.forEach(function (message) { merged[message.id] = message; });
      state.messages = Object.keys(merged).map(function (key) { return merged[key]; }).sort(function (a, b) { return a.id - b.id; });
      if (older || !before && state.messages.length <= result.messages.length) state.hasMore = result.has_more;
      renderMessages(forceBottom); if (older) $('message-list').scrollTop = oldTop + $('message-list').scrollHeight - oldHeight; show($('chat-error'), false);
    } catch (error) { if (request === chatRequest) { $('chat-error').querySelector('span').textContent = error.message; show($('chat-error'), true); } }
    finally { if (request === chatRequest) state.loading = false; }
  }
  async function send() {
    var text = $('message-input').value.trim(), chat = state.chat; if (!text || !chat) return;
    $('send-button').disabled = true;
    try { await api(chatPath(chat) + '/send', { text: text }); if (state.chat && state.chat.id === chat.id) { if ($('message-input').value.trim() === text) $('message-input').value = ''; await loadMessages(false, true); setFocus($('message-input')); } loadChats(); }
    catch (error) { toast(error.message); }
    finally { $('send-button').disabled = false; }
  }
  var stickers = { catalog: null, tab: 'recent', request: 0, sending: false, chat: null, pack: null, packs: new Map() };
  async function openStickers() {
    if (!state.chat || stickers.sending) return;
    stickers.chat = state.chat; stickers.catalog = null; stickers.packs.clear(); stickers.tab = 'recent'; stickers.pack = null;
    show($('stickers'), true); $('sticker-button').setAttribute('aria-expanded', 'true'); setFocus($('stickers-close'));
    var request = ++stickers.request; stickerStatus(t('Loading…')); updateStickerTabs();
    try { var catalog = await api('/stickers'); if (request !== stickers.request) return; stickers.catalog = catalog; renderStickers(); }
    catch (error) { if (request === stickers.request) stickerStatus(error.message, openStickers); }
  }
  function closeStickers(focus) {
    stickers.request++; show($('stickers'), false); $('sticker-button').setAttribute('aria-expanded', 'false');
    if (focus !== false) setFocus($('sticker-button'));
  }
  function updateStickerTabs() {
    ['recent', 'favorites', 'sets'].forEach(function (tab) { var selected = stickers.tab === tab; $('stickers-' + tab).disabled = !stickers.catalog; $('stickers-' + tab).classList.toggle('selected', selected); $('stickers-' + tab).setAttribute('aria-pressed', String(selected)); });
    $('stickers-title').textContent = stickers.pack ? normalize(stickers.pack.title) : t('Choose a sticker'); show($('stickers-back'), Boolean(stickers.pack));
  }
  function stickerStatus(text, retry) {
    var grid = $('sticker-grid'); grid.textContent = ''; grid.classList.remove('pack-list'); grid.appendChild(el('p', 'list-empty', text));
    if (retry) { var button = el('button', 'pill load-older', t('Retry')); button.onclick = retry; grid.appendChild(button); }
  }
  function renderStickers() {
    updateStickerTabs(); var catalog = stickers.catalog; if (!catalog) return;
    if (catalog.errors && catalog.errors[stickers.tab] && !stickers.pack) { stickerStatus(t('The operation failed. Please try again.'), openStickers); return; }
    var grid = $('sticker-grid'); grid.textContent = ''; grid.classList.toggle('pack-list', stickers.tab === 'sets' && !stickers.pack);
    if (stickers.tab === 'sets' && !stickers.pack) {
      if (!catalog.sets.length) { stickerStatus(t('Add sticker packs in Telegram on your phone to find them here.')); return; }
      catalog.sets.forEach(function (pack) { var button = el('button', 'sticker-pack', normalize(pack.title)); button.appendChild(el('span', '', String(pack.count))); button.onclick = function () { openStickerPack(pack); }; grid.appendChild(button); }); return;
    }
    var items = stickers.pack ? stickers.packs.get(stickers.pack.id) || [] : catalog[stickers.tab] || [];
    if (!items.length) { stickerStatus(t(stickers.pack ? 'This pack has no stickers.' : stickers.tab === 'favorites' ? 'No favorite stickers yet.' : 'No recent stickers. Choose a pack to get started.')); return; }
    items.forEach(function (item) {
      var button = el('button', 'sticker-choice', item.emoji || t('Sticker')); button.dataset.stickerId = item.id; button.setAttribute('aria-label', t('Send sticker {emoji}', { emoji: item.emoji || t('Sticker') })); button.disabled = stickers.sending;
      if (item.preview) { var img = el('img'); img.alt = ''; img.loading = 'lazy'; img.decoding = 'async'; img.src = resource('/stickers/' + encodeURIComponent(item.id) + '/thumbnail'); img.onload = function () { button.firstChild.nodeValue = ''; }; img.onerror = function () { img.remove(); }; button.appendChild(img); }
      button.onclick = function () { sendSticker(item); }; grid.appendChild(button);
    });
  }
  async function openStickerPack(pack) {
    stickers.pack = pack; updateStickerTabs();
    if (stickers.packs.has(pack.id)) { renderStickers(); setFocus($('sticker-grid').querySelector('button')); return; }
    var request = ++stickers.request; stickerStatus(t('Loading…'));
    try { var result = await api('/stickers/sets/' + encodeURIComponent(pack.id)); if (request !== stickers.request) return; stickers.packs.set(pack.id, result.stickers); renderStickers(); setFocus($('sticker-grid').querySelector('button')); }
    catch (error) { if (request === stickers.request) stickerStatus(error.message, function () { openStickerPack(pack); }); }
  }
  async function sendSticker(item) {
    if (stickers.sending || !stickers.chat) return;
    var chat = stickers.chat; stickers.sending = true;
    $('sticker-grid').querySelectorAll('button').forEach(function (node) { node.disabled = true; });
    try {
      await api(chatPath(chat) + '/stickers', { sticker_id: item.id });
      closeStickers(); toast(t('Sticker sent'));
      if (state.chat && state.chat.id === chat.id) await loadMessages(false, true);
      loadChats();
    } catch (error) { toast(error.message); }
    finally { stickers.sending = false; $('sticker-grid').querySelectorAll('button').forEach(function (node) { node.disabled = false; }); }
  }
  async function pressBotButton(message, chat, button, row, col, node) {
    node.disabled = true;
    try { var result = await api(mediaPath(chat, message) + '/button', { row: button.row === undefined ? row : button.row, col: button.col === undefined ? col : button.col }); if (result.message) toast(result.message); if (result.kind === 'miniapp' && result.url) openMiniapp(result.url); else if (result.url) openLink(result.url); if (state.chat && state.chat.id === chat.id) await loadMessages(); }
    catch (error) { toast(error.message); } finally { node.disabled = false; }
  }
  function safeHTTPS(url) { try { var parsed = new URL(url); return parsed.protocol === 'https:' && !parsed.username && !parsed.password ? parsed : null; } catch (_) { return null; } }
  function openLink(url) { var parsed = safeHTTPS(url); if (!parsed) { toast(t("This link is not supported.")); return; } if (parsed.hostname === 't.me' || parsed.hostname === 'telegram.me') openTelegramLink(parsed); else window.open(parsed.href, '_blank', 'noopener,noreferrer'); }
  async function openTelegramLink(url) {
    var parts = url.pathname.replace(/\/$/, '').slice(1).split('/'), username = parts[0], start = url.searchParams.get('start');
    if (url.port || !/^[A-Za-z][A-Za-z0-9_]{4,31}$/.test(username) || parts.length > 2 || (start !== null && !/^[A-Za-z0-9_-]{0,64}$/.test(start))) { toast(t("This Telegram link is not supported.")); return; }
    if (url.searchParams.has('startapp') || (parts.length === 2 && /^[A-Za-z][A-Za-z0-9_]{0,63}$/.test(parts[1]))) { if (start !== null) { toast(t("This Telegram link is not supported.")); return; } resolveMiniapp(url.href); return; }
    if (parts.length !== 1 || Array.from(url.searchParams.keys()).some(function (key) { return key !== 'start'; })) { toast(t("Telegram message and invitation links are not supported yet.")); return; }
    function hasDraft() { if (start !== null && $('message-input').value.trim()) { toast(t("You have a draft. Send or clear it before requesting content from the bot.")); return true; } return false; }
    if (hasDraft()) return;
    try {
      var chat = state.chats.find(function (item) { return item.username && item.username.toLowerCase() === username.toLowerCase(); });
      if (!chat) chat = (await api('/chats/resolve?username=' + encodeURIComponent(username))).chat;
      if (!chat) { toast(t("This Telegram chat could not be opened.")); return; }
      if (start !== null && chat.kind !== 'bot') { toast(t("This start link does not identify a bot.")); return; }
      if (hasDraft()) return;
      if (!$('miniapp').hidden) closeMiniapp();
      await openChat(chat);
      if (start !== null && state.chat && state.chat.id === chat.id && !hasDraft()) { $('message-input').value = '/start' + (start ? ' ' + start : ''); setFocus($('send-button')); toast(t("Press Send to request content from the bot.")); }
    } catch (error) { toast(t("Could not open the Telegram chat.") + " " + error.message); }
  }
  async function resolveMiniapp(url) { try { var result = await api('/miniapp', { url: url }); openMiniapp(result.url); } catch (error) { toast(error.message); } }
  function saveResume() { if (state.videoKey && Number.isFinite($('video').currentTime)) { try { localStorage.setItem('telesera-resume-' + state.videoKey, String($('video').ended ? 0 : $('video').currentTime)); } catch (_) {} } }
  function openVideo(message, chat) {
    state.returnFocus = document.activeElement; state.videoKey = chat.id + '-' + message.id; state.lastProgress = 0; state.lastTime = 0; show($('player'), true); show($('player-error'), false); $('player-title').textContent = message.media.name || t("Video"); $('video').src = resource(mediaPath(chat, message) + '/media'); $('video').load(); setFocus($('toggle-play')); var promise = $('video').play(); if (promise) promise.catch(function () { updatePlay(); });
  }
  function wait(ms) { return new Promise(function (resolve) { setTimeout(resolve, ms); }); }
  function liveError(message) { $('player-error').querySelector('p').textContent = message; show($('player-error'), true); setFocus($('retry-video')); }
  // Telegram serves live streams as one-second parts; the engine returns each as a self-contained fMP4 segment.
  async function openLive(chat) {
    if (!window.MediaSource) { toast(t("Live streams are not supported on this TV.")); return; }
    var video = $('video'), run = ++liveRun, source = new MediaSource(), buffer = null, next = 0, step = 1000, failures = 0;
    if ($('player').hidden) state.returnFocus = document.activeElement;
    state.videoKey = ''; state.liveChat = chat; state.lastProgress = 0; state.lastTime = 0; $('player').classList.add('live'); show($('player'), true); show($('player-error'), false); $('player-title').textContent = normalize(chat.title) + ' · ' + t("Live"); video.src = URL.createObjectURL(source); setFocus($('toggle-play')); var start = video.play(); if (start) start.catch(updatePlay);
    await new Promise(function (resolve) { source.addEventListener('sourceopen', resolve, { once: true }); }); URL.revokeObjectURL(video.src);
    function append(data) { return new Promise(function (resolve, reject) { buffer.onupdateend = resolve; buffer.onerror = reject; buffer.appendBuffer(data); }); }
    function trim(start, end) { return new Promise(function (resolve) { buffer.onupdateend = resolve; buffer.remove(start, end); }); }
    async function sync() { var info = await api(chatPath(chat) + '/live'); step = info.step_ms; next = info.time_ms - 2 * step; }
    // A part can take about as long to download as it lasts, so up to three are fetched ahead and appended in order.
    async function part(time) {
      for (var attempt = 0; ; attempt++) {
        var response = await fetch('/api' + chatPath(chat) + '/live/segment?t=' + time, { headers: { Authorization: 'Bearer ' + token }, cache: 'no-store' });
        if (response.status === 425 && attempt < 20) { await wait(500); continue; }
        if (response.status === 410) return { expired: true };
        if (!response.ok) { var body = await response.json().catch(function () { return {}; }), failure = new Error(apiError(body.error, response.status)); failure.final = response.status === 404; throw failure; }
        return { data: await response.arrayBuffer(), codecs: response.headers.get('x-codecs') };
      }
    }
    var pending = [];
    while (run === liveRun) {
      try {
        if (!next) { pending = []; await sync(); }
        var ranges = video.buffered, last = ranges.length - 1;
        if (last >= 0 && ranges.end(last) - video.currentTime > 6) { await wait(500); continue; }
        while (pending.length < 3) { var request = part(next); request.catch(function () {}); pending.push(request); next += step; }
        var result = await pending.shift(); if (run !== liveRun) return;
        if (result.expired) { next = 0; continue; }
        if (!buffer) buffer = source.addSourceBuffer('video/mp4; codecs="' + result.codecs + '"');
        await append(result.data); failures = 0; ranges = video.buffered; last = ranges.length - 1;
        // Jump to the newest range after a resync or when playback falls outside the buffer.
        if (video.currentTime < ranges.start(last) || video.currentTime > ranges.end(last)) { video.currentTime = ranges.start(last); var promise = video.play(); if (promise) promise.catch(updatePlay); }
        if (video.currentTime - ranges.start(0) > 30) await trim(ranges.start(0), video.currentTime - 10);
      } catch (error) {
        if (run !== liveRun) return;
        if (error.final || ++failures >= 5) { liveError(error.message || t("Playback is unavailable. Please retry.")); return; }
        // Drop the queue and resynchronise with the live edge after a failed part.
        next = 0; await wait(1000 * failures);
      }
    }
  }
  function closeVideo() { liveRun++; $('player').classList.remove('live'); saveResume(); $('video').pause(); $('video').removeAttribute('src'); $('video').load(); show($('player'), false); state.videoKey = ''; setFocus(state.returnFocus); }
  function updatePlay() { var paused = $('video').paused; $('toggle-play').textContent = paused ? '▶' : 'Ⅱ'; $('toggle-play').setAttribute('aria-label', paused ? t("Play") : t("Pause")); }
  function togglePlay() { if ($('video').paused) { var promise = $('video').play(); if (promise) promise.catch(function () { toast(t("Playback is unavailable. Please retry.")); }); } else $('video').pause(); }
  function seek(seconds) { var video = $('video'); if (!$('player').classList.contains('live') && Number.isFinite(video.duration)) { video.currentTime = Math.max(0, Math.min(video.duration, video.currentTime + seconds)); saveResume(); } }
  function openMiniapp(url) {
    var parsed = safeHTTPS(url); if (!parsed) { toast(t("The app did not return a secure link.")); return; }
    state.returnFocus = document.activeElement; state.miniOrigin = parsed.origin; state.miniBack = false; show($('miniapp-back'), false); show($('miniapp-footer'), false); show($('miniapp'), true); show($('miniapp-loading'), true); $('miniapp-title').textContent = state.chat ? normalize(state.chat.title) : 'Mini App'; $('miniapp-frame').src = parsed.href; setFocus($('miniapp-close'));
  }
  function closeMiniapp() { $('miniapp-frame').src = 'about:blank'; show($('miniapp'), false); state.miniOrigin = ''; state.lastActivity = Date.now(); setFocus(state.returnFocus); }
  function miniEvent(name, data) { if (state.miniOrigin) $('miniapp-frame').contentWindow.postMessage(JSON.stringify({ eventType: name, eventData: data || {} }), state.miniOrigin); }
  window.addEventListener('message', function (event) {
    if ($('miniapp').hidden || event.source !== $('miniapp-frame').contentWindow || event.origin !== state.miniOrigin) return;
    var data; try { data = typeof event.data === 'string' ? JSON.parse(event.data) : event.data; } catch (_) { return; } if (!data || typeof data.eventType !== 'string') return;
    var detail = data.eventData || {};
    switch (data.eventType) {
      case 'web_app_ready': show($('miniapp-loading'), false); break;
      case 'web_app_close': closeMiniapp(); break;
      case 'web_app_open_tg_link': if (typeof detail.path_full !== 'string' || !/^\/(?!\/)/.test(detail.path_full) || /[\\\u0000-\u0020]/.test(detail.path_full)) { toast(t("This Telegram link is not supported.")); break; } openLink('https://t.me' + detail.path_full); break;
      case 'web_app_open_link': var link = safeHTTPS(detail.url); if (link && (link.hostname === 't.me' || link.hostname === 'telegram.me')) openTelegramLink(link); else toast(t("The external link was not opened on the TV.")); break;
      case 'web_app_expand': miniEvent('viewport_changed', { height: $('miniapp-frame').clientHeight, is_state_stable: true, is_expanded: true }); break;
      case 'web_app_request_viewport': miniEvent('viewport_changed', { height: $('miniapp-frame').clientHeight, is_state_stable: true, is_expanded: true }); break;
      case 'web_app_request_theme': miniEvent('theme_changed', { theme_params: miniTheme }); break;
      case 'web_app_setup_back_button': state.miniBack = Boolean(detail.is_visible); show($('miniapp-back'), state.miniBack); break;
      case 'web_app_setup_main_button': $('miniapp-main').textContent = String(detail.text || t("Continue")).slice(0, 128); $('miniapp-main').disabled = detail.is_active === false; show($('miniapp-footer'), Boolean(detail.is_visible)); break;
      case 'web_app_request_fullscreen': miniEvent('fullscreen_changed', { is_fullscreen: true }); break;
      case 'web_app_exit_fullscreen': miniEvent('fullscreen_changed', { is_fullscreen: false }); break;
      case 'web_app_read_text_from_clipboard': miniEvent('clipboard_text_received', { req_id: detail.req_id }); break;
      case 'web_app_request_write_access': miniEvent('write_access_requested', { status: 'cancelled' }); break;
      case 'web_app_request_phone': miniEvent('phone_requested', { status: 'cancelled' }); break;
    }
  });
  function focusScope() { if (!$('stickers').hidden) return $('stickers'); if (!$('about').hidden) return $('about'); if (!$('miniapp').hidden) return $('miniapp'); if (!$('player').hidden) return $('player'); if (!$('login').hidden) return $('login'); if (!$('connection').hidden) return $('connection'); return $('app'); }
  function visibleFocusable(scope) { return Array.prototype.filter.call(scope.querySelectorAll('button:not(:disabled),input:not(:disabled),textarea:not(:disabled),a[href],iframe,[tabindex="0"]'), function (node) { var rect = node.getBoundingClientRect(); return rect.width > 0 && rect.height > 0 && !node.closest('[hidden]'); }); }
  function moveFocus(direction) {
    var nodes = visibleFocusable(focusScope()), current = document.activeElement;
    if (nodes.indexOf(current) < 0) { setFocus(nodes[0]); return; }
    var from = current.getBoundingClientRect(), fx = from.left + from.width / 2, fy = from.top + from.height / 2, candidate = null, best = Infinity;
    nodes.forEach(function (node) { if (node === current) return; var rect = node.getBoundingClientRect(), dx = rect.left + rect.width / 2 - fx, dy = rect.top + rect.height / 2 - fy; var primary = direction === 'ArrowRight' ? dx : direction === 'ArrowLeft' ? -dx : direction === 'ArrowDown' ? dy : -dy; if (primary <= 1) return; var cross = direction === 'ArrowLeft' || direction === 'ArrowRight' ? Math.abs(dy) : Math.abs(dx); var overlaps = direction === 'ArrowLeft' || direction === 'ArrowRight' ? rect.bottom > from.top && rect.top < from.bottom : rect.right > from.left && rect.left < from.right; var score = primary + cross * (overlaps ? 0.3 : 3) + (overlaps ? 0 : 100); if (score < best) { best = score; candidate = node; } });
    if (candidate) { candidate.focus({ preventScroll: true }); candidate.scrollIntoView({ block: 'nearest', inline: 'nearest' }); }
  }
  function back() {
    if (!$('stickers').hidden) { if (stickers.pack) { stickers.request++; stickers.pack = null; renderStickers(); setFocus($('stickers-sets')); } else closeStickers(); }
    else if (!$('about').hidden) { show($('about'), false); setFocus($('about-button')); }
    else if (!$('miniapp').hidden) { if (state.miniBack) miniEvent('back_button_pressed'); else closeMiniapp(); }
    else if (!$('player').hidden) closeVideo();
    else if (document.activeElement.matches('input,textarea')) document.activeElement.blur();
    else { $('app').classList.remove('chat-open'); setFocus($('chat-list').querySelector('.selected') || $('chat-list').querySelector('button') || $('inbox-tab')); }
  }
  document.addEventListener('keydown', function (event) {
    if (!$('screensaver').hidden) { show($('screensaver'), false); state.lastActivity = Date.now(); event.preventDefault(); event.stopImmediatePropagation(); return; }
    state.lastActivity = Date.now(); state.lastInput = Date.now(); var key = event.key, editing = event.target.matches('input:not([type="range"]),textarea,[contenteditable="true"]');
    // The first key on hidden player controls only reveals them; Back and media keys act immediately.
    if ($('player').classList.contains('idle')) { $('player').classList.remove('idle'); if (!(key === 'Escape' || key === 'BrowserBack' || key === 'MediaPlayPause' || [461, 415, 19, 179].includes(event.keyCode))) { event.preventDefault(); return; } }
    if (key === 'Escape' || key === 'BrowserBack' || event.keyCode === 461) { event.preventDefault(); back(); return; }
    if (editing) { if (key === 'Enter' && !event.shiftKey && !event.isComposing) { event.preventDefault(); event.target.blur(); setFocus(event.target.id === 'login-input' ? $('login-next') : event.target.id === 'message-input' ? $('send-button') : $('chat-list').querySelector('button')); } return; }
    if (!$('player').hidden) { if (key === 'ArrowLeft' || key === 'ArrowRight') { event.preventDefault(); seek(key === 'ArrowLeft' ? -15 : 15); return; } if (key === ' ' || key === 'MediaPlayPause' || event.keyCode === 179) { event.preventDefault(); togglePlay(); return; } if (event.keyCode === 415) { if ($('video').paused) togglePlay(); return; } if (event.keyCode === 19) { $('video').pause(); return; } }
    if (/^Arrow(Up|Down|Left|Right)$/.test(key)) { event.preventDefault(); moveFocus(key); }
    if (key === 'Tab' && focusScope() !== $('app')) { var nodes = visibleFocusable(focusScope()), first = nodes[0], last = nodes[nodes.length - 1]; if (event.shiftKey && document.activeElement === first) { event.preventDefault(); setFocus(last); } else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); setFocus(first); } }
  }, true);
  ['pointerdown', 'mousemove', 'touchstart'].forEach(function (type) { document.addEventListener(type, function (event) { state.lastActivity = Date.now(); state.lastInput = Date.now(); $('player').classList.remove('idle'); if (!$('screensaver').hidden) { show($('screensaver'), false); event.preventDefault(); event.stopImmediatePropagation(); } }, { capture: true, passive: false }); });
  function updateClock() { var now = new Date(); $('clock').textContent = time(now); $('saver-clock').querySelector('time').textContent = time(now); $('saver-clock').querySelector('p').textContent = now.toLocaleDateString(locale, { weekday: 'long', day: 'numeric', month: 'long' }); var playing = !$('player').hidden && !$('video').paused && Date.now() - state.lastProgress < 20000; if (playing || !$('miniapp').hidden) state.lastActivity = Date.now(); $('player').classList.toggle('idle', playing && $('player-error').hidden && Date.now() - state.lastInput >= 4000); if (Date.now() - state.lastActivity >= 180000) show($('screensaver'), true); }
  $('sticker-button').onclick = openStickers; $('stickers-close').onclick = function () { closeStickers(); }; $('stickers-back').onclick = function () { stickers.request++; stickers.pack = null; renderStickers(); setFocus($('stickers-sets')); };
  ['recent', 'favorites', 'sets'].forEach(function (tab) { $('stickers-' + tab).onclick = function () { stickers.request++; stickers.tab = tab; stickers.pack = null; renderStickers(); }; });
  $('retry-connection').onclick = connect; $('login-next').onclick = login; $('login-back').onclick = function () { state.authStep = 'phone'; renderLogin(); }; $('search').addEventListener('input', renderChats); $('inbox-tab').onclick = function () { selectArchive(false); }; $('archive-tab').onclick = function () { selectArchive(true); }; $('send-button').onclick = send; $('retry-chat').onclick = function () { loadMessages(); }; $('chat-back').onclick = function () { $('app').classList.remove('chat-open'); setFocus($('chat-list').querySelector('.selected')); }; $('bot-app').onclick = function () { if (state.chat && state.chat.username) resolveMiniapp('https://t.me/' + encodeURIComponent(state.chat.username) + '?startapp'); };
  $('about-button').onclick = function () { show($('about'), true); setFocus($('about-close')); }; $('about-close').onclick = function () { show($('about'), false); setFocus($('about-button')); };
  $('close-player').onclick = closeVideo; $('toggle-play').onclick = togglePlay; $('seek-back').onclick = function () { seek(-15); }; $('seek-forward').onclick = function () { seek(15); }; $('watch-live').onclick = function () { if (state.chat) openLive(state.chat); }; $('retry-video').onclick = function () { if ($('player').classList.contains('live')) { openLive(state.liveChat); return; } show($('player-error'), false); $('video').load(); var promise = $('video').play(); if (promise) promise.catch(updatePlay); }; $('video-progress').addEventListener('input', function () { if (Number.isFinite($('video').duration)) { $('video').currentTime = Number(this.value) / 1000 * $('video').duration; saveResume(); } });
  $('video').addEventListener('loadedmetadata', function () { var resume = 0; try { resume = Number(localStorage.getItem('telesera-resume-' + state.videoKey)) || 0; } catch (_) {} if (resume > 0 && Number.isFinite(this.duration) && resume < this.duration - 5) this.currentTime = resume; $('video-duration').textContent = duration(this.duration); });
  $('video').addEventListener('timeupdate', function () { if (Math.abs(this.currentTime - state.lastTime) > .05 && !this.paused && this.readyState >= 2) { state.lastProgress = Date.now(); state.lastTime = this.currentTime; } $('video-time').textContent = duration(this.currentTime); $('video-duration').textContent = duration(this.duration); $('video-progress').value = Number.isFinite(this.duration) && this.duration ? Math.round(this.currentTime / this.duration * 1000) : 0; saveResume(); });
  ['play', 'pause', 'ended'].forEach(function (event) { $('video').addEventListener(event, updatePlay); }); $('video').addEventListener('error', function () { if ((state.videoKey || $('player').classList.contains('live')) && this.getAttribute('src')) { $('player-error').querySelector('p').textContent = t(this.error && this.error.code === 2 ? 'The video connection was interrupted. Check your network and retry.' : this.error && this.error.code === 3 ? 'The TV could not decode this video. Try another version of the file.' : this.error && this.error.code === 4 ? 'This video format is not supported by the TV. Try another version of the file.' : 'Playback is unavailable. Please retry.'); show($('player-error'), true); } });
  $('miniapp-close').onclick = closeMiniapp; $('miniapp-back').onclick = function () { miniEvent('back_button_pressed'); }; $('miniapp-main').onclick = function () { miniEvent('main_button_pressed'); }; $('miniapp-frame').addEventListener('load', function () { show($('miniapp-loading'), false); miniEvent('theme_changed', { theme_params: miniTheme }); miniEvent('viewport_changed', { height: this.clientHeight, is_state_stable: true, is_expanded: true }); });
  window.addEventListener('beforeunload', saveResume); document.addEventListener('visibilitychange', function () { if (!document.hidden) { state.lastActivity = Date.now(); if (!$('app').hidden) { loadChats(); loadMessages(); } else if (!$('connection').hidden) connect(); } });
  window.addEventListener('online', function () { if (!$('connection').hidden) connect(); else if (!$('app').hidden) { loadChats(); loadMessages(); } });
  setInterval(function () { if (!$('app').hidden && !document.hidden && $('player').hidden && $('miniapp').hidden) loadMessages(); }, 5000); setInterval(function () { if (!$('app').hidden && !document.hidden) loadChats(); }, 30000); setInterval(updateClock, 1000); setInterval(function () { if ($('screensaver').hidden) return; var saver = $('saver-clock'); saver.style.left = (20 + Math.random() * Math.max(0, innerWidth - saver.offsetWidth - 40)) + 'px'; saver.style.top = (20 + Math.random() * Math.max(0, innerHeight - saver.offsetHeight - 40)) + 'px'; }, 30000); updateClock(); connect();
}());
