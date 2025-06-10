(function() {
  const WS_URL = 'ws://localhost:8765';
  let ws;
  let lastMoves = [];
  let gameStarted = false;
  let observer = null;
  let myColor = null;

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
  }

  function connect() {
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
  function algebraicToSquareIndex(square, color) {
    const file = square.charCodeAt(0) - 'a'.charCodeAt(0); // a=0 ... h=7
    const rank = parseInt(square[1], 10) - 1; // 1=0 ... 8=7
    if (color === 'white') {
      return (7 - rank) * 8 + file; // chess.com: top-left=0
    } else {
      // inversion pour les noirs
      return rank * 8 + (7 - file);
    }
  }

  /**
   * Simule un mouvement drag&drop de la pièce sur Chess.com
   */
  function simulateMove(from, to) {
    const color = detectColor();
    const fromIdx = algebraicToSquareIndex(from, color);
    const toIdx = algebraicToSquareIndex(to, color);

    // Les pièces sont dans .piece[class*="square-XX"]
    const piece = document.querySelector(`.piece[class*="square-${fromIdx}"]`);
    // Les cases sont dans .board .square-XX
    const toSquare = document.querySelector(`.board .square-${toIdx}`);

    if (!piece || !toSquare) {
      alert(`Impossible de trouver la pièce (${from}) ou la case cible (${to})`);
      return;
    }

    function getCenter(el) {
      const r = el.getBoundingClientRect();
      return { x: r.left + r.width/2, y: r.top + r.height/2 };
    }
    const fromCenter = getCenter(piece);
    const toCenter = getCenter(toSquare);

    function fireMouseEvent(type, x, y, target) {
      const evt = new MouseEvent(type, {
        bubbles: true, cancelable: true,
        clientX: x, clientY: y, buttons: 1, // button 0 = main bouton
      });
      target.dispatchEvent(evt);
    }

    // Séquence typique : mousedown (sur pièce) -> mousemove (vers case) -> mouseup (sur case)
    fireMouseEvent('mousedown', fromCenter.x, fromCenter.y, piece);
    // On simule le mouvement en plusieurs steps pour plus de "réalisme"
    for (let t = 1; t <= 3; ++t) {
      const x = fromCenter.x + (toCenter.x - fromCenter.x) * t / 3;
      const y = fromCenter.y + (toCenter.y - fromCenter.y) * t / 3;
      fireMouseEvent('mousemove', x, y, piece);
    }
    fireMouseEvent('mouseup', toCenter.x, toCenter.y, toSquare);
  }

  function onMessage(evt) {
    try {
      const data = JSON.parse(evt.data);
      if (data.result) {
        alert(`Partie terminée : ${data.result}`);
        return;
      } 
      console.log(`Message reçu : ${evt.data}`);
      // Cas 1 : reçoit coup "d2d4" pour jouer
      if (typeof data === "string" && /^[a-h][1-8][a-h][1-8]$/.test(data)) {
        const from = data.slice(0, 2);
        const to = data.slice(2, 4);
        simulateMove(from, to);
      } else if (data.next_move && /^[a-h][1-8][a-h][1-8]$/.test(data.next_move)) {
        const from = data.next_move.slice(0, 2);
        const to = data.next_move.slice(2, 4);
        simulateMove(from, to);
      } else {
        alert(`Prochain coup reçu : ${evt.data}`);
      }
    } catch {
      alert(`Prochain coup reçu : ${evt.data}`);
    }
  }

})();
