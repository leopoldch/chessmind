document.getElementById('start-game').onclick = () => {
  // Send a message to the active tab's content script
  chrome.tabs.query({active: true, currentWindow: true}, (tabs) => {
    chrome.tabs.sendMessage(tabs[0].id, {action: 'start_game'});
    document.getElementById('status').textContent = "Game started!";
  });
};

document.getElementById('stop-game').onclick = () => {
  chrome.tabs.query({active: true, currentWindow: true}, (tabs) => {
    chrome.tabs.sendMessage(tabs[0].id, {action: 'stop_game'});
    document.getElementById('status').textContent = "Game stopped";
  });
};
