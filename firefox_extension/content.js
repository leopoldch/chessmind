(function() {
  const WS_URL = 'ws://localhost:8765';
  let ws;
  let lastMoves = [];

  if (!/^\/game\/[^\/]+$/.test(window.location.pathname)) return;

  function connect() {
    ws = new WebSocket(WS_URL);
    ws.addEventListener('open', () => {
      const myColor = detectColor();
      ws.send(JSON.stringify({ type: 'color', color: myColor }));
      // On envoie la première liste de coups
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


  // Cette fonction renvoie TOUS les coups sous forme de liste
  function getAllMoves() {
    const moves = [];
    document.querySelectorAll('.timestamps-with-base-time .main-line-row').forEach(row => {
      for (const side of ['white', 'black']) {
        const node = row.querySelector(`.${side}-move .node-highlight-content`);
        if (node) moves.push(parseMove(node));
      }
    });
    return moves;
  }

  // Cette fonction extrait la notation correcte d'un élément DOM (avec figurine ou non)
  function parseMove(node) {
    // Si le coup contient une figurine (span), on la prend, sinon c’est un pion
    const figurine = node.querySelector('.icon-font-chess');
    let piece = '';
    if (figurine) {
      piece = figurine.dataset.figurine || '';
      // Conversion figurine vers lettre : N = Cavalier, B = Fou, R = Tour, Q = Dame, K = Roi
    }
    // On enlève l’icône du texte et on garde la case (e.g. 'c3')
    // En supprimant tout ce qui n’est pas lettre/chiffre ou espace
    let casePart = node.textContent.replace(/[♔-♟]/g, '').trim().replace(/\s+/g, '');
    return (piece ? piece : '') + casePart;
  }

  // On envoie la liste de tous les coups, au besoin
  function sendMoves(moves) {
    if (JSON.stringify(moves) === JSON.stringify(lastMoves)) return;
    lastMoves = moves;
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: 'moves', moves }));
    }
  }

  // Nouvelle fonction pour n'envoyer QUE le coup nouvellement joué
  function sendNewMove(newMove) {
    if (ws.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify({ type: 'move', move: newMove }));
    }
  }

  function observeMoves() {
    const container = document.querySelector('.timestamps-with-base-time');
    if (!container) return;
    const observer = new MutationObserver(() => {
      // On prend la nouvelle liste
      const currentMoves = getAllMoves();
      // On cherche les nouveaux coups en comparant à la dernière liste envoyée
      for (let i = lastMoves.length; i < currentMoves.length; i++) {
        const move = currentMoves[i];
        sendNewMove(move); // Envoie immédiat
      }
      // On update la variable de suivi
      lastMoves = currentMoves;
    });
    observer.observe(container, { childList: true, subtree: true });
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

  connect();
})();
