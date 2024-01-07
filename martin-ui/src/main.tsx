import React, { createContext, RefObject } from "react";
import ReactDOM from "react-dom/client";
import App from "./components/app.tsx";
import "./index.css";
import styled  from "styled-components";
import 'maplibre-gl/dist/maplibre-gl.css';


export const RootContext = createContext<RefObject<HTMLDivElement> | null>(
  null,
);

import { IntlProvider } from "react-intl";
const GlobalStyle = styled.div`
  font-family: ff-clan-web-pro, "Helvetica Neue", Helvetica, sans-serif;
  font-weight: 400;
  font-size: 0.875em;
  line-height: 1.71429;

  *,
  *:before,
  *:after {
    -webkit-box-sizing: border-box;
    -moz-box-sizing: border-box;
    box-sizing: border-box;
  }

  ul {
    margin: 0;
    padding: 0;
  }

  li {
    margin: 0;
  }

  a {
    text-decoration: none;
    color: ${(props) => props.theme.labelColor};
  }
`;

const MartinUI = () => {
  const rootRef = React.useRef<HTMLDivElement | null>(null);
  return (
      <IntlProvider locale={"english"}>
        <RootContext.Provider value={rootRef}>
            <GlobalStyle ref={rootRef}>
              <App />
            </GlobalStyle>
        </RootContext.Provider>
      </IntlProvider>
  );
};
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <MartinUI />,
);
