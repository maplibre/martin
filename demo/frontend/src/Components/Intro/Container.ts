import styled from 'styled-components';

export default styled.div`
  display: flex;
  justify-content: flex-end;
  align-items: center;

  height: 75vh;
  padding: 7vw;
  color: #fff;

  @media (max-width: 500px) {
    justify-content: center;
  }
`;
