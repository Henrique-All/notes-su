import styled from 'styled-components'

export const NotesContent = styled.div`
  display: flex;
  flex-direction: column;
  width: 95%;
  align-items: center;
  margin-top: 50px;

  .main {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 90%;
    .header {
      display: flex;
      align-items: center;

      h1 {
        color: #ffffff;
        font-family: 'Poppins', sans-serif;
        font-weight: 700;
      }

      button {
        position: absolute;
        right: 0px;
        background-color: #176775;
        border: none;
        cursor: pointer;
        font-family: 'Poppins', sans-serif;
        color: #ffffff;
        padding: 10px;
        border-radius: 10px;
      }
    }
  }
`
