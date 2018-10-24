import styled from 'styled-components';

export default styled.a`
  display: inline-flex;
  align-content: center;

  padding: 10px;
  border: solid 1px #fff;
  font-size: 20px;
  color: #fff;
  text-decoration: none;
  
  cursor: pointer;

  &:hover{
    background-color: #0E0E1E;
    box-shadow: 3px 3px 0 rgba(115, 0, 255, 1);
    border-color: #0E0E1E;
  }

  img {
    margin-left: 10px;
  }
`;
