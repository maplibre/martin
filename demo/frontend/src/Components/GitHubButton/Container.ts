import styled from 'styled-components';

export default styled.a`
  display: inline-flex;
  align-content: center;

  padding: 10px;
  border: solid 1px #fff;
  font-size: 20px;
  color: #fff;
  text-decoration: none;
  
  background-color: transparent;
  
  cursor: pointer;

  &:hover{
    border-color: transparent;

    box-shadow: 3px 3px 0 rgba(115, 0, 255, 1);
  }

  img {
    margin-left: 10px;
  }
`;
