(function() {
  const WS_URL = 'ws://localhost:8765';
  let ws;
  let lastSentMoves = null;

  if (!/^\/game\/[^\/]+$/.test(window.location.pathname)){
    console.log('Pas une page de jeu, arrêt du script.');
    return;
  }

  function connect() {
    ws = new WebSocket(WS_URL);
    ws.addEventListener('open', () => {
      const myColor = detectColor();
      ws.send(JSON.stringify({ type: 'color', color: myColor }));
      sendMovesIfNew(getAllMoves());
      observeMoves();
    });
    ws.addEventListener('message', onMessage);
    ws.addEventListener('close', () => setTimeout(connect, 1000));
  }

  function detectColor() {
    const whiteKings = document.querySelectorAll('.piece.wk');
    return (whiteKings.length > 0) ? 'white' : 'black';
  }

  function getAllMoves() {
    const moves = [];
    document.querySelectorAll('.node.white-move, .node.black-move')
      .forEach(n => moves.push(n.textContent.trim()));
    return moves;
  }

  function sendMovesIfNew(moves) {
    const serialized = JSON.stringify(moves);
    if (serialized !== lastSentMoves) {
      lastSentMoves = serialized;
      ws.send(JSON.stringify({ type: 'moves', moves }));
    }
  }

  function observeMoves() {
    const container = document.querySelector('.mode-swap-move-list-component');
    if (!container) return;
    const obs = new MutationObserver(() => {
      sendMovesIfNew(getAllMoves());
    });
    obs.observe(container, { childList: true, subtree: true });
  }

  function onMessage(evt) {
    try {
      const data = JSON.parse(evt.data);
      if (data.result) {
        alert(`Partie terminée: ${data.result}`);
        return;
      }
    } catch {}
    alert(`Prochain coup reçu: ${evt.data}`);
  }

  // Démarrage
  connect();
})();
