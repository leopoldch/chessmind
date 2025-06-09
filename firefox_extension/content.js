// Content script for chess‑site extension (fixed)
(() => {
  const WS_URL = 'ws://localhost:8765';
  let ws;
  let lastMoveCount = 0;

  /** Wait for a DOM element to appear */
  function waitForElement(selector, timeout = 10000) {
    return new Promise(resolve => {
      const el = document.querySelector(selector);
      if (el) {
        resolve(el);
        return;
      }
      const observer = new MutationObserver(() => {
        const el = document.querySelector(selector);
        if (el) {
          observer.disconnect();
          resolve(el);
        }
      });
      observer.observe(document.documentElement, { childList: true, subtree: true });
      setTimeout(() => {
        observer.disconnect();
        resolve(null);
      }, timeout);
    });
  }

  /** Try to establish (or re‑establish) the WebSocket connection */
  function connect() {
    ws = new WebSocket(WS_URL);

    ws.addEventListener('open', async () => {
      const colour = await detectColour();
      if (colour) ws.send(colour); // e.g. "white" or "black"
      observeMoves(colour);
    });

    ws.addEventListener('message', event => {
      // Engine either sends a JSON (game over) or plain text long‑algebraic move
      try {
        const data = JSON.parse(event.data);
        if (data.result) {
          alert(`Game over: ${data.result}`);
          return;
        }
      } catch {
        // Not JSON ⇒ should be coordinate like "e7e5"
        if (event.data && event.data.length === 4) {
          makeEngineMove(event.data);
        }
      }
    });

    ws.addEventListener('close', () => {
      // simple reconnection strategy
      setTimeout(connect, 1000);
    });
  }

  /** Determine our colour by checking board orientation.  Works on Lichess & Chess.com */
  async function detectColour() {
    const board = await waitForElement('chess-board, .board');
    if (!board) return null;

    const isWhiteBottom =
      !board.classList.contains('flipped') ||
      board.getAttribute('orientation') === 'white';
    return isWhiteBottom ? 'black' : 'white';
  }

  /** Return array with SAN strings from the move list */
  function getMoveTexts() {
    const nodes = document.querySelectorAll('.vertical-move-list .move');
    return Array.from(nodes).map(n => n.textContent.trim()).filter(Boolean);
  }

  /** When move list mutates, send the newest opponent move to the backend */
  function sendLastOpponentMove(colour) {
    if (!ws || ws.readyState !== WebSocket.OPEN) return;

    const moves = getMoveTexts();
    if (moves.length === 0 || moves.length === lastMoveCount) return;
    lastMoveCount = moves.length;

    const isWhite = colour === 'white';
    const opponentMoves = moves.filter((_, i) => (isWhite ? i % 2 === 1 : i % 2 === 0));
    const last = opponentMoves[opponentMoves.length - 1];
    if (last) {
      ws.send(last); // send SAN like "e4", "Nf3" etc.
    }
  }

  /** Observe DOM change in the move list */
  async function observeMoves(colour) {
    const list = await waitForElement('.vertical-move-list');
    if (!list) return;

    // Send any move already present when we connected
    sendLastOpponentMove(colour);

    const observer = new MutationObserver(() => sendLastOpponentMove(colour));
    observer.observe(list, { childList: true, subtree: true });
  }

  /** Play engine move on the board (simple version using chessboard API if present) */
  function makeEngineMove(coord) {
    // If you have access to a board API (e.g. window.board.move) use it.
    // This placeholder just alerts.
    alert(`Engine plays: ${coord}`);
  }

  // Kick off
  connect();
})();