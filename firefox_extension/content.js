/*  content.js  – injecté dans https://www.chess.com/play …
 *  1. Détecte la couleur du joueur affiché en bas.
 *  2. Observe la liste des coups et expédie UNIQUEMENT les coups adverses.
 *  3. Tous les messages sont envoyés au serveur WS sous forme JSON
 *     { type: 'color' | 'move', payload: string }             */

(() => {
  const WS_URL = 'ws://localhost:8765';
  let ws;                       // WebSocket courant
  let myColor = null;           // 'white' ou 'black'
  let lastPlySent = 0;          // indice (½-coup) du dernier coup vraiment expédié

  /* ----------  OUTILS GÉNÉRIQUES  ---------- */

  /** Attend qu’un sélecteur CSS apparaisse dans le DOM */
  const waitForEl = (sel, timeout = 10000) =>
    new Promise((ok, ko) => {
      const el = document.querySelector(sel);
      if (el) return ok(el);
      const obs = new MutationObserver(() => {
        const found = document.querySelector(sel);
        if (found) { obs.disconnect(); ok(found); }
      });
      obs.observe(document.documentElement, { childList: true, subtree: true });
      setTimeout(() => { obs.disconnect(); ko(`Timeout : ${sel}`); }, timeout);
    });

  /** Envoie un objet JSON si le WS est ouvert */
  const send = obj => {
    if (ws?.readyState === WebSocket.OPEN) {
      ws.send(JSON.stringify(obj));
    }
  };

  /* ----------  DÉTECTION DE LA COULEUR  ----------
   * Astuce : le #board imprime les pièces comme sprites dans le style inline.
   * 'br.png' (tour noire) vient avant 'wr.png' (tour blanche) si le NOIR est EN HAUT
   * -> Donc si br < wr  →  je suis blanc en bas.                         */
  const detectColor = () => {
    const board = document.getElementById('board');
    if (!board) return null;
    const style = board.getAttribute('style') || '';
    const pBr = style.indexOf('br.png');
    const pWr = style.indexOf('wr.png');
    if (pBr === -1 || pWr === -1) return null;
    return pBr < pWr ? 'white' : 'black';
  };

  /** Wait until detectColor() returns a value */
  const waitForColor = async () => {
    await waitForEl('#board');
    return new Promise((resolve) => {
      const color = detectColor();
      if (color) return resolve(color);
      const board = document.getElementById('board');
      const obs = new MutationObserver(() => {
        const c = detectColor();
        if (c) { obs.disconnect(); resolve(c); }
      });
      obs.observe(board, { attributes: true, attributeFilter: ['style'] });
    });
  };

  /* ----------  OBSERVATION DES COUPS  ---------- */

  /** Retourne la liste plate des ½-coups dans l’ordre : ['d4','d5','c4',…] */
  const getMoves = (listNode) => {
    const moves = [];
    listNode.querySelectorAll('.move').forEach(node => {
      const white = node.querySelector('.white')?.textContent.trim();
      const black = node.querySelector('.black')?.textContent.trim();
      if (white) moves.push(white);
      if (black) moves.push(black);
    });
    return moves;
  };

  /** Envoie le dernier coup adverse s’il y en a un nouveau */
  const maybeSendOpponentMove = (listNode) => {
    const moves = getMoves(listNode);
    if (moves.length === 0 || moves.length === lastPlySent) return;

    const isWhiteBottom = myColor === 'white';
    const plyIdx   = moves.length - 1;
    const oppTurn  = isWhiteBottom ? plyIdx % 2 === 1 : plyIdx % 2 === 0;

    lastPlySent = moves.length;          // on mémorise le nouveau total

    if (oppTurn) {
      send({ type: 'move', payload: moves[plyIdx] });
    }
  };

  /** Installe un MutationObserver sur la liste verticale */
  const hookMoveList = async () => {
    const list = await waitForEl('.vertical-move-list');
    maybeSendOpponentMove(list);           // on rattrape la position courante

    new MutationObserver(() => maybeSendOpponentMove(list))
      .observe(list, { childList: true, subtree: true });
  };

  /* ----------  GESTION DU WEBSOCKET  ---------- */

  const connectWS = () => {
    ws = new WebSocket(WS_URL);

    ws.addEventListener('open', () => {
      send({ type: 'color', payload: myColor });
      hookMoveList().catch(console.error);
    });

    ws.addEventListener('close', () => setTimeout(connectWS, 1000));
  };

  /** Observe orientation changes and notify the server */
  const watchOrientation = () => {
    const board = document.getElementById('board');
    if (!board) return;
    new MutationObserver(() => {
      const c = detectColor();
      if (c && c !== myColor) {
        myColor = c;
        lastPlySent = 0;
        send({ type: 'color', payload: myColor });
      }
    }).observe(board, { attributes: true, attributeFilter: ['style'] });
  };

  /* ----------  INITIALISATION  ---------- */

  (async () => {
    myColor = await waitForColor();      // attend que la couleur soit détectable
    if (!myColor) return console.error('Impossible de déterminer la couleur.');

    connectWS();                         // et c’est parti !
    watchOrientation();
  })();

})();
