import { useState } from "react";
import "./App.css";
import { webClient } from "./webClient";

function App() {
  const [clientStarted, setClientStarted] = useState(false);

  const handleClick = () => {
    webClient();
    setClientStarted(true);
  };

  return (
    <div className="App">
      <h1>Miden Web App</h1>

      <p>Open the console to view logs</p>

      {!clientStarted && (
        <button onClick={handleClick}>Start Web Client</button>
      )}
    </div>
  );
}

export default App;
