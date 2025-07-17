import styled from 'styled-components';

export default styled.div`
  display: flex;
  align-items: center;
  justify-content: center;

  max-width: 980px;
  height: 50vh;
  padding: 7vw;
  margin:auto;

  font-size: 50px;
  line-height: 1.55;

  @media (max-width: 500px) {
    height: 30vh;
    margin-bottom: 150px;

    font-size: 30px;
  }
`;
