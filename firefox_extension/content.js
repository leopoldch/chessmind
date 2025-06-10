(function() {
  const WS_URL = 'ws://localhost:8765';
  let ws;
  let lastMoves = [];
  let gameStarted = false;
  let observer = null;
  let myColor = null;

  function parseMove(node) {
    if (node && node.dataset && node.dataset.uci) {
      return node.dataset.uci;
    }
    return node ? node.textContent.trim() : '';
  }

  if (!/^\/game\/[^\/]+$/.test(window.location.pathname)) return;

  // Écoute du message venant de la popup pour démarrer la partie
  chrome.runtime.onMessage.addListener((msg) => {
    if (msg.action === 'start_game') {
      startGame();
    }
  });

  function startGame() {
    if (gameStarted) return;
    gameStarted = true;
    connect();
    window.addEventListener('beforeunload', () => {
      if (ws) {
        try { ws.close(); } catch (e) {}
      }
    });
  }

  function connect() {
      if (ws && ws.readyState !== WebSocket.CLOSED) {
        try { ws.close(); } catch (e) {}
      }
      ws = new WebSocket(WS_URL);
      ws.addEventListener('open', () => {
          myColor = detectColor();
          ws.send(JSON.stringify({ type: 'color', color: myColor }));
          // Envoi immédiat de la liste des coups présents
          sendMoves(getAllMoves());
          observeMoves();
      });
      ws.addEventListener('message', onMessage);
      ws.addEventListener('close', () => setTimeout(connect, 1000));
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
        if (color !== myColor) { // On n’envoie que les coups de l’adversaire
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

  // ================================
  //  PARTIE PRINCIPALE À AJOUTER ICI
  // ================================

  /**
   * Convertit e2 (notation algébrique) en index de case Chess.com
   * Chess.com indexe les cases de 0 à 63 de haut-gauche à bas-droite pour les blancs
   * et inverse si on joue noir (plateau flipped)
   */
  function algebraicToSquareIndex(square) {
    const file = square.charCodeAt(0) - 'a'.charCodeAt(0) + 1; // a=1 ... h=8
    const rank = parseInt(square[1], 10); // 1 à 8
    return rank * 10 + file;
  }


function simulateMove(from, to) {
  // 1) Récupérer la pièce à déplacer
  const fromIdx = algebraicToSquareIndex(from);
  const piece = document.querySelector(`.piece[class*="square-${fromIdx}"]`);
  if (!piece) {
    alert(`Pièce introuvable sur ${from}`);
    return;
  }

  // 2) Récupérer le plateau (élément interactif)
  const board = document.querySelector('wc-chess-board.board');
  if (!board) {
    alert("Impossibilité de trouver le plateau");
    return;
  }
  const rect = board.getBoundingClientRect();
  const tileW = rect.width / 8;
  const tileH = rect.height / 8;

  // 3) Calculer les coordonnées pixel du centre de la case cible
  const file = to.charCodeAt(0) - 'a'.charCodeAt(0);     // 0 à 7
  const rank = parseInt(to[1], 10);                      // 1 à 8
  const orientation = detectColor();                     // 'white' ou 'black'

  let fileIdx, rankIdx;
  if (orientation === 'white') {
    fileIdx = file;
    rankIdx = 8 - rank;    // rank=1 → idx=7 (bas), rank=8 → idx=0 (haut)
  } else {
    fileIdx = 7 - file;    // miroir horizontal
    rankIdx = rank - 1;    // miroir vertical
  }

  const toX = rect.left + (fileIdx + 0.5) * tileW;
  const toY = rect.top  + (rankIdx + 0.5) * tileH;

  // 4) Séquence drag&drop
  const fromBox = piece.getBoundingClientRect();
  const fromX = fromBox.left + fromBox.width  / 2;
  const fromY = fromBox.top  + fromBox.height / 2;

  function fireMouse(type, x, y, target) {
    const evt = new MouseEvent(type, {
      bubbles: true, cancelable: true,
      clientX: x, clientY: y, buttons: 1
    });
    target.dispatchEvent(evt);
  }

  fireMouse('mousedown', fromX, fromY, piece);
  // quelques mousemove intermédiaires pour plus de réalisme
  for (let t = 1; t <= 3; t++) {
    const ix = fromX + (toX - fromX) * t / 3;
    const iy = fromY + (toY - fromY) * t / 3;
    fireMouse('mousemove', ix, iy, board);
  }
  fireMouse('mouseup', toX, toY, board);
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
