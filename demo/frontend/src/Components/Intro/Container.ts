import styled from 'styled-components';

export default styled.div`
  display: flex;
  justify-content: flex-end;
  align-items: center;

  height: 110vh;
  padding: 7vw;

  color: #fff;

  background: url('public/logo.png') no-repeat;
  background-size: 500px;
  background-position: 50px 50px;

  @media (max-width: 500px) {
    justify-content: center;
    background-size: 250px;
    background-position: 25px 25px;
  }
`;
