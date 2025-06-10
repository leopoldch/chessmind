(function() {
  const port = 8771;
  const WS_URL = 'ws://localhost:'+port;
  let ws;
  let lastMoves = [];
  let gameStarted = false;
  let observer = null;
  let myColor = null;
  let shouldReconnect = false;

  function parseMove(node) {
    if (node && node.dataset && node.dataset.uci) {
      return node.dataset.uci;
    }

    const figurine = node ? node.querySelector('.icon-font-chess') : null;
    let piece = '';
    if (figurine && figurine.dataset) {
      piece = figurine.dataset.figurine || '';
    }
    const rawText = node ? node.textContent : '';
    const casePart = rawText.replace(/[\u2654-\u265F]/g, '').trim().replace(/\s+/g, '');
    if (piece || casePart) {
      return (piece ? piece : '') + casePart;
    }
    return '';
  }

  if (!/^\/game\/[^\/]+$/.test(window.location.pathname)) return;

  chrome.runtime.onMessage.addListener((msg) => {
    if (msg.action === 'start_game') {
      startGame();
    } else if (msg.action === 'stop_game') {
      stopGame();
    }
  });

  function startGame() {
    if (gameStarted) return;
    gameStarted = true;
    shouldReconnect = true;
    connect();
    window.addEventListener('beforeunload', stopGame);
  }

  function stopGame() {
    shouldReconnect = false;
    gameStarted = false;
    if (observer) {
      observer.disconnect();
      observer = null;
    }
    if (ws) {
      try { ws.close(); } catch (e) {}
      ws = null;
    }
  }

  function connect() {
      if (!shouldReconnect) return;
      if (ws && ws.readyState !== WebSocket.CLOSED) {
        try { ws.close(); } catch (e) {}
      }
      ws = new WebSocket(WS_URL);
      ws.addEventListener('open', () => {
          myColor = detectColor();
          ws.send(JSON.stringify({ type: 'color', color: myColor }));
          sendMoves(getAllMoves());
          observeMoves();
      });
      ws.addEventListener('message', onMessage);
      ws.addEventListener('close', () => {
        ws = null;
        if (shouldReconnect) setTimeout(connect, 1000);
      });
      ws.addEventListener('error', () => {
        if (ws && ws.readyState !== WebSocket.CLOSED) {
          ws.close();
        }
      });
  }

  function detectColor() {
    const svg = document.querySelector('svg.coordinates');
    if (!svg) return null;
    const text1 = Array.from(svg.querySelectorAll('text')).find(t => t.textContent.trim() === "1");
    const text8 = Array.from(svg.querySelectorAll('text')).find(t => t.textContent.trim() === "8");
    if (!text1 || !text8) return null;
    const y1 = parseFloat(text1.getAttribute('y'));
    const y8 = parseFloat(text8.getAttribute('y'));
    if (y1 > y8) return 'white';
    return 'black';
  }

  function getAllMoves() {
    const moves = [];
    document.querySelectorAll('.timestamps-with-base-time .main-line-row').forEach(row => {
      for (const side of ['white', 'black']) {
        const node = row.querySelector(`.${side}-move .node-highlight-content`);
        if (node) moves.push({ move: parseMove(node), color: side });
      }
    });
    return moves;
  }

  function sendMoves(moves) {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    if (JSON.stringify(moves) === JSON.stringify(lastMoves)) return;
    lastMoves = moves;
    ws.send(JSON.stringify({ type: 'moves', moves }));
  }

  function sendNewMove(newMove) {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;
    ws.send(JSON.stringify({ type: 'move', move: newMove }));
  }

  function observeMoves() {
    const container = document.querySelector('.timestamps-with-base-time');
    if (!container) return;
    if (observer) observer.disconnect();

    observer = new MutationObserver(() => {
      const currentMoves = getAllMoves();
      for (let i = lastMoves.length; i < currentMoves.length; i++) {
        const { move, color } = currentMoves[i];
        if (color !== myColor) {
          sendNewMove(move);
        }
      }
      lastMoves = currentMoves;
    });

    observer.observe(container, { childList: true, subtree: true });
    const currentMoves = getAllMoves();
    for (let i = lastMoves.length; i < currentMoves.length; i++) {
      const { move, color } = currentMoves[i];
      if (color !== myColor) {
        sendNewMove(move);
      }
    }
    lastMoves = currentMoves;
  }


  function algebraicToSquareIndex(square) {
    const file = square.charCodeAt(0) - 'a'.charCodeAt(0) + 1; 
    const rank = parseInt(square[1], 10);
    return file * 10 + rank; 
  }

function clickSquare(idx, square) {
  const board = document.querySelector('wc-chess-board.board');
  const rect  = board.getBoundingClientRect();
  const file  = square.charCodeAt(0) - 'a'.charCodeAt(0); 
  const rank  = parseInt(square[1], 10); 
  const orientation = detectColor();
  let fileIdx = orientation==='white' ? file : 7 - file;
  let rankIdx = orientation==='white' ? 8 - rank : rank - 1;
  const x = rect.left + (fileIdx + 0.5) * (rect.width  / 8);
  const y = rect.top  + (rankIdx + 0.5) * (rect.height / 8);

  const target = document.elementFromPoint(x, y);

  if (!target) {
    console.warn("Rien sous le point", x, y);
    return;
  }

  const seq = [
    { type: 'pointerdown', ctor: PointerEvent },
    { type: 'mousedown',   ctor: MouseEvent   },
    { type: 'pointerup',   ctor: PointerEvent },
    { type: 'mouseup',     ctor: MouseEvent   },
    { type: 'click',       ctor: MouseEvent   },
  ];

  for (const {type, ctor} of seq) {
    const evt = new ctor(type, {
      bubbles: true, cancelable: true,
      clientX: x, clientY: y,
      pointerId: 1, pointerType: 'mouse', isPrimary: true,
      buttons: (type.includes('down') ? 1 : 0)
    });
    target.dispatchEvent(evt);
  }
}

  function simulateMove(from, to) {
    const fromIdx = algebraicToSquareIndex(from);
    const toIdx   = algebraicToSquareIndex(to);
    clickSquare(fromIdx, from);
    setTimeout(() => clickSquare(toIdx, to), 100);
  }



  function onMessage(evt) {
    let data;
    try {
      data = JSON.parse(evt.data);
    } catch {
      data = evt.data;
    }

    if (typeof data === 'object' && data !== null) {
      if (data.result) {
        alert(`Partie terminée : ${data.result}`);
        return;
      }
      if (data.next_move && /^[a-h][1-8][a-h][1-8]$/.test(data.next_move)) {
        const from = data.next_move.slice(0, 2);
        const to = data.next_move.slice(2, 4);
        simulateMove(from, to);
        return;
      }
      alert(`Prochain coup reçu : ${evt.data}`);
      return;
    }

    if (typeof data === 'string' && /^[a-h][1-8][a-h][1-8]$/.test(data)) {
      const from = data.slice(0, 2);
      const to = data.slice(2, 4);
      simulateMove(from, to);
    } else {
      alert(`Prochain coup reçu : ${evt.data}`);
    }
  }

})();
