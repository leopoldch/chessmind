(function() {
  const WS_URL = 'ws://localhost:8765';
  let ws;
  let lastMoveCount = 0;

  function connect() {
    ws = new WebSocket(WS_URL);
    ws.addEventListener('open', () => {
      const color = detectColor();
      if (color) ws.send(color); // send plain text color
      observeMoves(color);
    });
    ws.addEventListener('message', event => {
      try {
        const data = JSON.parse(event.data);
        if (data.result) {
          alert(`Game over: ${data.result}`);
          return;
        }
      } catch {
        // not JSON, treat as next move
        if (event.data) {
          alert(`Next move: ${event.data}`);
        }
      }
    });
    ws.addEventListener('close', () => {
      setTimeout(connect, 1000);
    });
  }

  function detectColor() {
    const board = document.getElementById('board');
    if (!board) return null;
    const style = board.getAttribute('style') || '';
    const br = style.indexOf('br.png');
    const wr = style.indexOf('wr.png');
    if (br === -1 || wr === -1) return null;
    return br < wr ? 'white' : 'black';
  }

  function getMoveTexts() {
    const nodes = document.querySelectorAll('.vertical-move-list .move');
    return Array.from(nodes).map(n => n.textContent.trim()).filter(Boolean);
  }

  function sendLastOpponentMove(color) {
    const moves = getMoveTexts();
    if (moves.length === 0 || moves.length === lastMoveCount) return;
    lastMoveCount = moves.length;
    const isWhite = color === 'white';
    const opponentMoves = moves.filter((_, i) => isWhite ? i % 2 === 1 : i % 2 === 0);
    const last = opponentMoves[opponentMoves.length - 1];
    if (last && ws.readyState === WebSocket.OPEN) {
      ws.send(last); // send move in plain text
    }
  }

  function observeMoves(color) {
    const list = document.querySelector('.vertical-move-list');
    if (!list) return;
    sendLastOpponentMove(color);
    const observer = new MutationObserver(() => sendLastOpponentMove(color));
    observer.observe(list, { childList: true, subtree: true });
  }

  connect();
})();
