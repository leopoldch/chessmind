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


  function parseMove(node) {
    const figurine = node.querySelector('.icon-font-chess');
    let piece = '';
    if (figurine) piece = figurine.dataset.figurine || '';
    let casePart = node.textContent.replace(/[♔-♟]/g, '').trim().replace(/\s+/g, '');
    return (piece ? piece : '') + casePart;
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
      // Détection des nouveaux coups uniquement
      for (let i = lastMoves.length; i < currentMoves.length; i++) {
        const { move, color } = currentMoves[i];
        if (color !== myColor) { // On n’envoie que les coups de l’adversaire
          sendNewMove(move);
        }
      }
      lastMoves = currentMoves;
    });

    observer.observe(container, { childList: true, subtree: true });
    // A l'init, envoie aussi les coups de l'adversaire déjà joués
    const currentMoves = getAllMoves();
    for (let i = lastMoves.length; i < currentMoves.length; i++) {
      const { move, color } = currentMoves[i];
      if (color !== myColor) {
        sendNewMove(move);
      }
    }
    lastMoves = currentMoves;
  }


  function onMessage(evt) {
    try {
      const data = JSON.parse(evt.data);
      if (data.result) {
        alert(`Partie terminée : ${data.result}`);
        return;
      }
    } catch {}
    alert(`Prochain coup reçu : ${evt.data}`);
  }

  // Ne pas lancer connect tant que la popup n'a pas dit "start_game"
  // connect(); // <-- On ne connecte que sur signal de la popup !
})();
