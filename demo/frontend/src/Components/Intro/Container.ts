import styled from 'styled-components';

import logoMartin from './logo_martin.svg';

export default styled.div`
  display: flex;
  justify-content: flex-end;
  align-items: center;

  height: 110vh;
  padding: 7vw;

  color: #fff;

  background: url(${logoMartin}) no-repeat;
  background-size: contain;

  @media (max-width: 500px) {
    justify-content: center;
  }
`;
