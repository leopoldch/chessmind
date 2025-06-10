document.getElementById('start-game').onclick = () => {
  // Envoie un message au content-script de l’onglet actif
  chrome.tabs.query({active: true, currentWindow: true}, (tabs) => {
    chrome.tabs.sendMessage(tabs[0].id, {action: 'start_game'});
    document.getElementById('status').textContent = "Partie démarrée !";
  });
};

document.getElementById('stop-game').onclick = () => {
  chrome.tabs.query({active: true, currentWindow: true}, (tabs) => {
    chrome.tabs.sendMessage(tabs[0].id, {action: 'stop_game'});
    document.getElementById('status').textContent = "Partie stoppée";
  });
};
